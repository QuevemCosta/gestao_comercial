use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use uuid::Uuid;

use crate::domain::deliverer::{Deliverer, DelivererData};
use crate::domain::delivery::{
    Delivery, DeliveryAssignment, DeliveryHistory, DeliveryStatus, ScheduleEntry, StatusChange,
};
use crate::domain::errors::DomainError;
use crate::domain::order::{OrderStatus, ReceiveMode};
use crate::domain::repositories::DeliveryFilter;
use crate::domain::sale::SaleStatus;
use crate::domain::value_objects::Money;

use super::clock::Clock;
use super::error::AppError;
use super::unit_of_work::{Transactor, UnitOfWork};

/// Janela do painel para "próximas entregas".
const UPCOMING_WINDOW_HOURS: i64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDeliveryInput {
    pub contact_phone: String,
    pub fee: Money,
    pub notes: String,
    pub scheduled_for: Option<NaiveDateTime>,
}

/// Indicadores do painel de entregas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryPanel {
    pub today: usize,
    pub upcoming: usize,
    pub overdue: usize,
    pub awaiting_deliverer: usize,
    pub assigned: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub not_delivered: usize,
    pub cancelled: usize,
}

/// Casos de uso de entregadores e entregas. Nenhum deles altera pagamento da venda.
pub struct Deliveries<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Deliveries<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Deliveries<'_, S> {
    pub fn register_deliverer(&mut self, data: DelivererData) -> Result<Deliverer, AppError> {
        let now = self.clock.now();
        let deliverer = Deliverer::create(Uuid::now_v7(), data, now)?;
        let mut uow = self.store.begin()?;
        uow.insert_deliverer(&deliverer)?;
        uow.commit()?;
        Ok(deliverer)
    }

    pub fn update_deliverer(&mut self, id: Uuid, data: DelivererData) -> Result<Deliverer, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut deliverer = require_deliverer(&*uow, id)?;
        deliverer.update(data, now)?;
        uow.update_deliverer(&deliverer)?;
        uow.commit()?;
        Ok(deliverer)
    }

    pub fn set_deliverer_active(&mut self, id: Uuid, active: bool) -> Result<Deliverer, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut deliverer = require_deliverer(&*uow, id)?;
        deliverer.set_active(active, now);
        uow.update_deliverer(&deliverer)?;
        uow.commit()?;
        Ok(deliverer)
    }

    pub fn get_deliverer(&mut self, id: Uuid) -> Result<Deliverer, AppError> {
        let uow = self.store.begin()?;
        let deliverer = require_deliverer(&*uow, id)?;
        uow.commit()?;
        Ok(deliverer)
    }

    pub fn list_deliverers(&mut self, only_active: bool) -> Result<Vec<Deliverer>, AppError> {
        let uow = self.store.begin()?;
        let deliverers = uow.list_deliverers(only_active)?;
        uow.commit()?;
        Ok(deliverers)
    }

    /// Cria a entrega a partir de um pedido confirmado com modalidade de entrega.
    /// O endereço é copiado do pedido. Se houver data, a entrega já nasce agendada.
    pub fn create_for_order(&mut self, order_id: Uuid, input: NewDeliveryInput) -> Result<Delivery, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;

        let order = uow
            .find_order(order_id)?
            .ok_or(AppError::NotFound("pedido"))?;
        if order.receive_mode() != ReceiveMode::Delivery {
            return Err(DomainError::DeliveryNotRequired.into());
        }
        if order.status() != OrderStatus::Confirmed {
            return Err(DomainError::OrderNotConfirmed.into());
        }
        match uow.sale_status_for_order(order_id)? {
            None => return Err(DomainError::OrderNotConfirmed.into()),
            Some(SaleStatus::Cancelled) => return Err(DomainError::CancelledSaleNoDelivery.into()),
            Some(_) => {}
        }
        if uow.find_delivery_by_order(order_id)?.is_some() {
            return Err(DomainError::DeliveryAlreadyExists.into());
        }

        let destination = order
            .delivery_address()
            .cloned()
            .ok_or(DomainError::DeliveryAddressRequired)?;

        let mut delivery = Delivery::create(
            Uuid::now_v7(),
            order_id,
            order.customer_id(),
            destination,
            &input.contact_phone,
            input.fee,
            &input.notes,
            now,
        )?;

        uow.insert_delivery(&delivery)?;
        uow.record_status_change(&StatusChange::opening(delivery.id(), now))?;

        if let Some(when) = input.scheduled_for {
            apply_schedule(&mut *uow, &mut delivery, when, now)?;
        }

        uow.commit()?;
        Ok(delivery)
    }

    pub fn get(&mut self, id: Uuid) -> Result<Delivery, AppError> {
        let uow = self.store.begin()?;
        let delivery = require_delivery(&*uow, id)?;
        uow.commit()?;
        Ok(delivery)
    }

    pub fn history(&mut self, id: Uuid) -> Result<DeliveryHistory, AppError> {
        let uow = self.store.begin()?;
        require_delivery(&*uow, id)?;
        let history = uow.delivery_history(id)?;
        uow.commit()?;
        Ok(history)
    }

    pub fn schedule(&mut self, id: Uuid, when: NaiveDateTime) -> Result<Delivery, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut delivery = require_delivery(&*uow, id)?;
        apply_schedule(&mut *uow, &mut delivery, when, now)?;
        uow.commit()?;
        Ok(delivery)
    }

    /// Atribui a entrega a um entregador ativo. Uma atribuição anterior é encerrada,
    /// preservando o histórico.
    pub fn assign(&mut self, id: Uuid, deliverer_id: Uuid) -> Result<DeliveryAssignment, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;

        let delivery = require_delivery(&*uow, id)?;
        if delivery.is_closed() {
            return Err(DomainError::DeliveryClosed.into());
        }

        let deliverer = require_deliverer(&*uow, deliverer_id)?;
        if !deliverer.is_active() {
            return Err(DomainError::DelivererInactive.into());
        }

        if let Some(mut current) = uow.active_assignment(id)? {
            if current.deliverer_id() == deliverer_id {
                return Err(DomainError::AlreadyAssignedToDeliverer.into());
            }
            current.close(now)?;
            uow.update_assignment(&current)?;
        }

        let assignment = DeliveryAssignment::open(Uuid::now_v7(), id, deliverer_id, now);
        uow.insert_assignment(&assignment)?;
        uow.commit()?;
        Ok(assignment)
    }

    pub fn depart(&mut self, id: Uuid) -> Result<Delivery, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut delivery = require_delivery(&*uow, id)?;
        let has_assignment = uow.active_assignment(id)?.is_some();
        let change = delivery.depart(has_assignment, now)?;
        persist_change(&mut *uow, &delivery, &change)?;
        uow.commit()?;
        Ok(delivery)
    }

    pub fn complete(&mut self, id: Uuid, received_by: &str, notes: &str) -> Result<Delivery, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut delivery = require_delivery(&*uow, id)?;
        let change = delivery.complete(received_by, notes, now)?;
        persist_change(&mut *uow, &delivery, &change)?;
        uow.commit()?;
        Ok(delivery)
    }

    pub fn fail(&mut self, id: Uuid, reason: &str) -> Result<Delivery, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut delivery = require_delivery(&*uow, id)?;
        let change = delivery.fail(reason, now)?;
        persist_change(&mut *uow, &delivery, &change)?;
        uow.commit()?;
        Ok(delivery)
    }

    pub fn cancel(&mut self, id: Uuid, reason: &str) -> Result<Delivery, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut delivery = require_delivery(&*uow, id)?;
        cancel_delivery(&mut *uow, &mut delivery, reason, now)?;
        uow.commit()?;
        Ok(delivery)
    }

    pub fn search(&mut self, filter: &DeliveryFilter) -> Result<Vec<Delivery>, AppError> {
        let uow = self.store.begin()?;
        let deliveries = uow.search_deliveries(filter)?;
        uow.commit()?;
        Ok(deliveries)
    }

    /// Entregas agendadas para o dia, em ordem de horário.
    pub fn daily_agenda(&mut self, day: NaiveDate) -> Result<Vec<Delivery>, AppError> {
        let from = day.and_time(NaiveTime::MIN);
        self.agenda_between(from, from + Duration::days(1))
    }

    /// Entregas agendadas para a semana iniciada em `week_start` (sete dias).
    pub fn weekly_agenda(&mut self, week_start: NaiveDate) -> Result<Vec<Delivery>, AppError> {
        let from = week_start.and_time(NaiveTime::MIN);
        self.agenda_between(from, from + Duration::days(7))
    }

    fn agenda_between(&mut self, from: NaiveDateTime, to: NaiveDateTime) -> Result<Vec<Delivery>, AppError> {
        self.search(&DeliveryFilter {
            scheduled_from: Some(from),
            scheduled_to: Some(to),
            ..DeliveryFilter::default()
        })
    }

    /// Indicadores do painel, calculados em uma única unidade de trabalho.
    pub fn panel(&mut self) -> Result<DeliveryPanel, AppError> {
        let now = self.clock.now();
        let today = now.date();
        let window = Duration::hours(UPCOMING_WINDOW_HOURS);

        let uow = self.store.begin()?;
        let deliveries = uow.search_deliveries(&DeliveryFilter::default())?;
        let mut panel = DeliveryPanel::default();

        for delivery in &deliveries {
            let assigned = uow.active_assignment(delivery.id())?.is_some();

            match delivery.status() {
                DeliveryStatus::Cancelled => panel.cancelled += 1,
                DeliveryStatus::Delivered => panel.completed += 1,
                DeliveryStatus::NotDelivered => panel.not_delivered += 1,
                DeliveryStatus::OutForDelivery => panel.in_progress += 1,
                DeliveryStatus::AwaitingScheduling | DeliveryStatus::Scheduled => {}
            }

            if delivery.is_due_on(today) && delivery.status() != DeliveryStatus::Cancelled {
                panel.today += 1;
            }
            if delivery.is_closed() {
                continue;
            }
            if assigned {
                panel.assigned += 1;
            } else {
                panel.awaiting_deliverer += 1;
            }
            if delivery.is_upcoming(now, window) {
                panel.upcoming += 1;
            }
            if delivery.is_overdue(now) {
                panel.overdue += 1;
            }
        }

        uow.commit()?;
        Ok(panel)
    }
}

/// Usado pelo cancelamento de venda. Entregas em andamento ou concluídas bloqueiam o cancelamento.
/// Entregas abertas são canceladas e a atribuição ativa é encerrada, na mesma unidade de trabalho.
pub fn cancel_for_order_cancellation(
    uow: &mut dyn UnitOfWork,
    order_id: Uuid,
    now: NaiveDateTime,
) -> Result<(), AppError> {
    let Some(mut delivery) = uow.find_delivery_by_order(order_id)? else {
        return Ok(());
    };
    match delivery.status() {
        DeliveryStatus::Cancelled => Ok(()),
        DeliveryStatus::OutForDelivery => Err(DomainError::DeliveryInProgress.into()),
        DeliveryStatus::Delivered => Err(DomainError::DeliveryAlreadyCompleted.into()),
        _ => cancel_delivery(uow, &mut delivery, "venda cancelada", now),
    }
}

fn apply_schedule(
    uow: &mut dyn UnitOfWork,
    delivery: &mut Delivery,
    when: NaiveDateTime,
    now: NaiveDateTime,
) -> Result<(), AppError> {
    let change = delivery.schedule(when, now)?;
    uow.update_delivery(&*delivery)?;
    uow.record_schedule(&ScheduleEntry {
        delivery_id: delivery.id(),
        scheduled_for: when,
        recorded_at: now,
    })?;
    if let Some(change) = change {
        uow.record_status_change(&change)?;
    }
    Ok(())
}

fn cancel_delivery(
    uow: &mut dyn UnitOfWork,
    delivery: &mut Delivery,
    reason: &str,
    now: NaiveDateTime,
) -> Result<(), AppError> {
    let change = delivery.cancel(reason, now)?;
    if let Some(mut assignment) = uow.active_assignment(delivery.id())? {
        assignment.close(now)?;
        uow.update_assignment(&assignment)?;
    }
    persist_change(uow, delivery, &change)
}

fn persist_change(
    uow: &mut dyn UnitOfWork,
    delivery: &Delivery,
    change: &StatusChange,
) -> Result<(), AppError> {
    uow.update_delivery(delivery)?;
    uow.record_status_change(change)?;
    Ok(())
}

fn require_deliverer(uow: &dyn UnitOfWork, id: Uuid) -> Result<Deliverer, AppError> {
    uow.find_deliverer(id)?
        .ok_or(AppError::NotFound("entregador"))
}

fn require_delivery(uow: &dyn UnitOfWork, id: Uuid) -> Result<Delivery, AppError> {
    uow.find_delivery(id)?
        .ok_or(AppError::NotFound("entrega"))
}