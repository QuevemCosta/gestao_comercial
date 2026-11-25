use chrono::{Datelike, NaiveDateTime};
use uuid::Uuid;

use crate::domain::audit::AuditEntry;
use crate::domain::errors::DomainError;
use crate::domain::payment::{status_for, Payment, PaymentMethod, PaymentStatus};
use crate::domain::rental::{
    available_for, ensure_available, format_rental_number, peak_demand, rental_sequence_name,
    Commitment, Period, Rental, RentalLine, RentalStatus, ReturnCount, ReturnEntry, ReturnEvent,
};
use crate::domain::rental_item::{RentalItem, RentalItemData};
use crate::domain::repositories::RentalFilter;
use crate::domain::validation::required_text;
use crate::domain::value_objects::Money;

use super::clock::Clock;
use super::error::AppError;
use super::unit_of_work::{Transactor, UnitOfWork};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalLineInput {
    pub rental_item_id: Uuid,
    pub quantity: i64,
    pub unit_price: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnLineInput {
    pub line_id: Uuid,
    pub returned: i64,
    pub damaged: i64,
    pub lost: i64,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalSummary {
    pub rental: Rental,
    pub total: Money,
    pub received: Money,
    pub balance: Money,
    pub payment_status: PaymentStatus,
    pub outstanding_units: i64,
    pub late: bool,
}

/// Casos de uso de locação: itens, reservas, retiradas, devoluções e pagamentos.
pub struct Rentals<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Rentals<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Rentals<'_, S> {
    pub fn register_item(&mut self, data: RentalItemData, initial_total: i64) -> Result<RentalItem, AppError> {
        let now = self.clock.now();
        let item = RentalItem::create(Uuid::now_v7(), data, initial_total, now)?;
        let mut uow = self.store.begin()?;
        uow.insert_rental_item(&item)?;
        uow.commit()?;
        Ok(item)
    }

    pub fn update_item(&mut self, id: Uuid, data: RentalItemData) -> Result<RentalItem, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut item = require_item(&*uow, id)?;
        item.update(data, now)?;
        uow.update_rental_item(&item)?;
        uow.commit()?;
        Ok(item)
    }

    pub fn set_item_active(&mut self, id: Uuid, active: bool) -> Result<RentalItem, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut item = require_item(&*uow, id)?;
        item.set_active(active, now);
        uow.update_rental_item(&item)?;
        uow.commit()?;
        Ok(item)
    }

    pub fn add_units(&mut self, id: Uuid, quantity: i64, reason: &str) -> Result<RentalItem, AppError> {
        let now = self.clock.now();
        let reason = required_text("motivo", reason)?;
        let mut uow = self.store.begin()?;
        let mut item = require_item(&*uow, id)?;
        item.add_units(quantity, now)?;
        uow.update_rental_item(&item)?;
        uow.record_audit(&audit(now, "entrada_locacao", id, quantity, &reason))?;
        uow.commit()?;
        Ok(item)
    }

    /// Baixa definitiva de unidades. Bloqueada se deixar alguma reserva sem capacidade.
    pub fn write_off(&mut self, id: Uuid, quantity: i64, reason: &str) -> Result<RentalItem, AppError> {
        let now = self.clock.now();
        let reason = required_text("motivo", reason)?;
        let mut uow = self.store.begin()?;
        let mut item = require_item(&*uow, id)?;
        item.write_off(quantity, now)?;

        let commitments = commitments_for_item(&*uow, id, now)?;
        let required = peak_demand(&commitments);
        if item.total_quantity() < required {
            return Err(DomainError::WriteOffBlockedByReservations { required }.into());
        }

        uow.update_rental_item(&item)?;
        uow.record_audit(&audit(now, "baixa_locacao", id, quantity, &reason))?;
        uow.commit()?;
        Ok(item)
    }

    pub fn get_item(&mut self, id: Uuid) -> Result<RentalItem, AppError> {
        let uow = self.store.begin()?;
        let item = require_item(&*uow, id)?;
        uow.commit()?;
        Ok(item)
    }

    pub fn list_items(&mut self, only_active: bool) -> Result<Vec<RentalItem>, AppError> {
        let uow = self.store.begin()?;
        let items = uow.list_rental_items(only_active)?;
        uow.commit()?;
        Ok(items)
    }

    /// Unidades disponíveis de um item em um período, considerando todas as locações abertas.
    pub fn availability(&mut self, item_id: Uuid, start: NaiveDateTime, end: NaiveDateTime) -> Result<i64, AppError> {
        let now = self.clock.now();
        let period = Period::new(start, end)?;
        let uow = self.store.begin()?;
        let item = require_item(&*uow, item_id)?;
        let commitments = commitments_for_item(&*uow, item_id, now)?;
        uow.commit()?;
        Ok(available_for(item.total_quantity(), &commitments, &period))
    }

    /// Cria uma reserva. A disponibilidade é verificada dentro da mesma unidade de trabalho que grava a locação.
    pub fn create(
        &mut self,
        customer_id: Uuid,
        pickup_at: NaiveDateTime,
        expected_return_at: NaiveDateTime,
        discount: Money,
        inputs: Vec<RentalLineInput>,
    ) -> Result<Rental, AppError> {
        let now = self.clock.now();
        let period = Period::new(pickup_at, expected_return_at)?;

        let mut uow = self.store.begin()?;
        let customer = uow
            .find_customer(customer_id)?
            .ok_or(AppError::NotFound("cliente"))?;
        if !customer.is_active() {
            return Err(DomainError::CustomerInactive.into());
        }

        let open = uow.open_rentals()?;
        let commitments: Vec<Commitment> = open
            .iter()
            .flat_map(|rental| rental.commitments(now))
            .collect();

        let mut lines = Vec::with_capacity(inputs.len());
        for input in inputs {
            let item = require_item(&*uow, input.rental_item_id)?;
            if !item.is_active() {
                return Err(DomainError::RentalItemInactive.into());
            }
            let line = RentalLine::new(
                Uuid::now_v7(),
                input.rental_item_id,
                input.quantity,
                input.unit_price,
            )?;
            let booked: Vec<Commitment> = commitments
                .iter()
                .filter(|c| c.rental_item_id == input.rental_item_id)
                .copied()
                .collect();
            let available = available_for(item.total_quantity(), &booked, &period);
            ensure_available(available, line.quantity())?;
            lines.push(line);
        }

        let year = now.year();
        let sequence = uow.next_sequence_value(&rental_sequence_name(year))?;
        let number = format_rental_number(year, sequence);
        let rental = Rental::reserve(
            Uuid::now_v7(),
            number,
            customer_id,
            period,
            discount,
            lines,
            now,
        )?;
        uow.insert_rental(&rental)?;
        uow.commit()?;
        Ok(rental)
    }

    pub fn pick_up(&mut self, rental_id: Uuid) -> Result<Rental, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut rental = require_rental(&*uow, rental_id)?;
        rental.pick_up(now)?;
        uow.update_rental(&rental)?;
        uow.commit()?;
        Ok(rental)
    }

    pub fn cancel(&mut self, rental_id: Uuid, reason: &str) -> Result<Rental, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut rental = require_rental(&*uow, rental_id)?;
        rental.cancel(reason, now)?;
        uow.update_rental(&rental)?;
        uow.commit()?;
        Ok(rental)
    }

    pub fn register_return(&mut self, rental_id: Uuid, inputs: Vec<ReturnLineInput>) -> Result<Rental, AppError> {
        let now = self.clock.now();
        let entries: Vec<ReturnEntry> = inputs
            .into_iter()
            .map(|input| ReturnEntry {
                line_id: input.line_id,
                count: ReturnCount {
                    returned: input.returned,
                    damaged: input.damaged,
                    lost: input.lost,
                },
                notes: input.notes.trim().to_owned(),
            })
            .collect();

        let mut uow = self.store.begin()?;
        let mut rental = require_rental(&*uow, rental_id)?;
        rental.apply_return(&entries, now)?;

        let events: Vec<ReturnEvent> = entries
            .into_iter()
            .map(|entry| ReturnEvent {
                id: Uuid::now_v7(),
                line_id: entry.line_id,
                occurred_at: now,
                count: entry.count,
                notes: entry.notes,
            })
            .collect();

        uow.update_rental(&rental)?;
        uow.insert_return_events(&events)?;
        uow.commit()?;
        Ok(rental)
    }

    pub fn register_payment(
        &mut self,
        rental_id: Uuid,
        method: PaymentMethod,
        amount: Money,
        notes: &str,
    ) -> Result<Payment, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let rental = require_rental(&*uow, rental_id)?;
        rental.ensure_accepts_payments()?;

        let payment = Payment::for_rental(Uuid::now_v7(), rental_id, method, amount, now, notes)?;

        let payments = uow.payments_for_rental(rental_id)?;
        let received = sum_received(&payments)?;
        let balance = rental.total()?.checked_sub(received)?;
        if amount > balance {
            return Err(DomainError::PaymentExceedsBalance { balance }.into());
        }

        uow.insert_payment(&payment)?;
        uow.commit()?;
        Ok(payment)
    }

    pub fn summary(&mut self, rental_id: Uuid) -> Result<RentalSummary, AppError> {
        let now = self.clock.now();
        let uow = self.store.begin()?;
        let rental = require_rental(&*uow, rental_id)?;
        let payments = uow.payments_for_rental(rental_id)?;
        uow.commit()?;

        let received = sum_received(&payments)?;
        let total = rental.total()?;
        let balance = total.checked_sub(received)?;
        let outstanding_units = rental.outstanding_units();
        let late = rental.is_late(now);

        Ok(RentalSummary {
            rental,
            total,
            received,
            balance,
            payment_status: status_for(total, received),
            outstanding_units,
            late,
        })
    }

    pub fn get(&mut self, rental_id: Uuid) -> Result<Rental, AppError> {
        let uow = self.store.begin()?;
        let rental = require_rental(&*uow, rental_id)?;
        uow.commit()?;
        Ok(rental)
    }

    pub fn list(&mut self, filter: &RentalFilter) -> Result<Vec<Rental>, AppError> {
        let uow = self.store.begin()?;
        let rentals = uow.search_rentals(filter)?;
        uow.commit()?;
        Ok(rentals)
    }
}

fn require_item(uow: &dyn UnitOfWork, id: Uuid) -> Result<RentalItem, AppError> {
    uow.find_rental_item(id)?
        .ok_or(AppError::NotFound("item de locação"))
}

fn require_rental(uow: &dyn UnitOfWork, id: Uuid) -> Result<Rental, AppError> {
    uow.find_rental(id)?
        .ok_or(AppError::NotFound("locação"))
}

/// Compromissos abertos de um item. Locações canceladas e encerradas não aparecem.
fn commitments_for_item(uow: &dyn UnitOfWork, item_id: Uuid, now: NaiveDateTime) -> Result<Vec<Commitment>, AppError> {
    let open = uow.open_rentals()?;
    Ok(open
        .iter()
        .flat_map(|rental| rental.commitments(now))
        .filter(|c| c.rental_item_id == item_id)
        .collect())
}

fn audit(now: NaiveDateTime, action: &str, item_id: Uuid, quantity: i64, reason: &str) -> AuditEntry {
    AuditEntry {
        occurred_at: now,
        action: action.to_owned(),
        entity: "item_locacao".to_owned(),
        details: format!("item={item_id}; quantidade={quantity}; motivo={reason}"),
    }
}

fn sum_received(payments: &[Payment]) -> Result<Money, DomainError> {
    payments
        .iter()
        .try_fold(Money::ZERO, |acc, payment| acc.checked_add(payment.amount()))
}

/// Mantém a importação usada apenas para documentação de tipos públicos.
#[allow(dead_code)]
fn _status_marker(status: RentalStatus) -> RentalStatus {
    status
}