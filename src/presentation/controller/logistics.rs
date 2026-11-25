//! Telas de locações, entregas e entregadores. Estende o `Controller` com as
//! ações e consultas dessas áreas, sempre por meio dos casos de uso.

use std::collections::HashSet;

use chrono::{NaiveDate, NaiveDateTime};
use slint::SharedString;
use uuid::Uuid;

use crate::application::deliveries::{Deliveries, NewDeliveryInput};
use crate::application::orders::Orders;
use crate::application::rentals::{Rentals, RentalLineInput, ReturnLineInput};
use crate::application::sales::Sales;
use crate::domain::deliverer::{Deliverer, DelivererData};
use crate::domain::delivery::DeliveryStatus;
use crate::domain::order::{Order, OrderStatus, ReceiveMode};
use crate::domain::rental::RentalStatus;
use crate::domain::rental_item::{RentalItem, RentalItemData};
use crate::domain::repositories::{DeliveryFilter, OrderFilter, RentalFilter, SaleFilter};
use crate::domain::sale::SaleStatus;
use crate::domain::value_objects::Money;
use crate::presentation::format::{parse_int, parse_money, plain_money};
use crate::presentation::{
    DashCard, DelivererRow, DeliveryRow, DraftLineRow, RentalItemRow, RentalLineRow, RentalRow,
};

use super::{
    card, msg, non_empty, parse_id, payment_label, payment_method_from_index, Controller,
};

// ---------- Formulários e visões ----------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RentalItemForm {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub price: String,
    pub notes: String,
    /// Usado somente no cadastro; em edição é ignorado.
    pub initial_total: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RentalForm {
    /// Índice entre os clientes ativos; -1 significa "nenhum".
    pub customer_index: i32,
    pub pickup: String,
    pub expected: String,
    pub discount: String,
}

/// Linha do rascunho de locação, mantida pela própria tela.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftLine {
    pub item_id: String,
    pub quantity: String,
    pub price: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DelivererForm {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub document: String,
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewDeliveryForm {
    /// Índice entre os pedidos disponíveis para entrega; -1 significa "nenhum".
    pub order_index: i32,
    pub phone: String,
    pub fee: String,
    pub notes: String,
    pub scheduled: String,
}

pub struct RentalView {
    pub title: String,
    pub summary: String,
    pub lines: Vec<RentalLineRow>,
    pub line_options: Vec<SharedString>,
}

pub struct DeliveryView {
    pub title: String,
    pub details: String,
    pub history: Vec<SharedString>,
}

// ---------- Funções auxiliares ----------

pub fn parse_datetime(field: &str, raw: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(raw.trim(), "%d/%m/%Y %H:%M")
        .map_err(|_| format!("informe '{field}' no formato dd/mm/aaaa hh:mm"))
}

fn parse_date(field: &str, raw: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(raw.trim(), "%d/%m/%Y")
        .map_err(|_| format!("informe '{field}' no formato dd/mm/aaaa"))
}

fn fmt_dt(value: NaiveDateTime) -> String {
    value.format("%d/%m/%Y %H:%M").to_string()
}

fn rental_status_text(status: RentalStatus, late: bool) -> &'static str {
    match status {
        RentalStatus::Reserved => "Reservada",
        RentalStatus::Active if late => "Atrasada",
        RentalStatus::Active => "Retirada",
        RentalStatus::Closed => "Encerrada",
        RentalStatus::Cancelled => "Cancelada",
    }
}

fn delivery_status_text(status: DeliveryStatus) -> &'static str {
    match status {
        DeliveryStatus::AwaitingScheduling => "Aguardando agendamento",
        DeliveryStatus::Scheduled => "Agendada",
        DeliveryStatus::OutForDelivery => "Saiu para entrega",
        DeliveryStatus::Delivered => "Entregue",
        DeliveryStatus::NotDelivered => "Não entregue",
        DeliveryStatus::Cancelled => "Cancelada",
    }
}

fn item_name(items: &[RentalItem], id: Uuid) -> String {
    items
        .iter()
        .find(|item| item.id() == id)
        .map(|item| item.data().name.clone())
        .unwrap_or_else(|| "—".to_owned())
}

fn deliverer_name(deliverers: &[Deliverer], id: Uuid) -> String {
    deliverers
        .iter()
        .find(|deliverer| deliverer.id() == id)
        .map(|deliverer| deliverer.data().name.clone())
        .unwrap_or_else(|| "—".to_owned())
}

fn rental_item_row(item: &RentalItem) -> RentalItemRow {
    let data = item.data();
    let status = if item.is_active() { "Ativo" } else { "Inativo" };
    RentalItemRow {
        id: item.id().to_string().into(),
        name: data.name.as_str().into(),
        category: data.category.as_str().into(),
        price: data.rental_price.to_string().into(),
        total: item.total_quantity().to_string().into(),
        status: status.into(),
    }
}

fn deliverer_row(deliverer: &Deliverer) -> DelivererRow {
    let data = deliverer.data();
    let status = if deliverer.is_active() { "Ativo" } else { "Inativo" };
    DelivererRow {
        id: deliverer.id().to_string().into(),
        name: data.name.as_str().into(),
        phone: data.phone.as_str().into(),
        document: data.document.clone().unwrap_or_default().into(),
        status: status.into(),
    }
}

fn rental_line_input(line: &DraftLine) -> Result<RentalLineInput, String> {
    Ok(RentalLineInput {
        rental_item_id: parse_id(&line.item_id)?,
        quantity: parse_int("quantidade", &line.quantity)?,
        unit_price: parse_money("valor unitário", &line.price)?,
    })
}

// ---------- Ações do controlador ----------

impl Controller {
    // ----- Itens de locação -----

    fn all_rental_items(&mut self) -> Result<Vec<RentalItem>, String> {
        Rentals::new(&mut self.db, &self.clock)
            .list_items(false)
            .map_err(msg)
    }

    fn active_rental_items(&mut self) -> Result<Vec<RentalItem>, String> {
        Rentals::new(&mut self.db, &self.clock)
            .list_items(true)
            .map_err(msg)
    }

    pub fn rental_item_rows(&mut self) -> Result<Vec<RentalItemRow>, String> {
        Ok(self.all_rental_items()?.iter().map(rental_item_row).collect())
    }

    pub fn rental_item_options(&mut self) -> Result<Vec<SharedString>, String> {
        Ok(self
            .active_rental_items()?
            .iter()
            .map(|item| SharedString::from(item.data().name.as_str()))
            .collect())
    }

    pub fn save_rental_item(&mut self, form: RentalItemForm) -> Result<String, String> {
        let data = RentalItemData {
            name: form.name,
            description: form.description,
            category: form.category,
            rental_price: parse_money("valor da locação", &form.price)?,
            notes: form.notes,
        };
        if form.id.trim().is_empty() {
            let initial = parse_int("quantidade total", &form.initial_total)?;
            Rentals::new(&mut self.db, &self.clock)
                .register_item(data, initial)
                .map_err(msg)?;
            Ok("Item de locação cadastrado.".to_owned())
        } else {
            let id = parse_id(&form.id)?;
            Rentals::new(&mut self.db, &self.clock)
                .update_item(id, data)
                .map_err(msg)?;
            Ok("Item de locação atualizado.".to_owned())
        }
    }

    pub fn load_rental_item(&mut self, item_id: &str) -> Result<RentalItemForm, String> {
        let id = parse_id(item_id)?;
        let item = Rentals::new(&mut self.db, &self.clock)
            .get_item(id)
            .map_err(msg)?;
        let data = item.data();
        Ok(RentalItemForm {
            id: item.id().to_string(),
            name: data.name.clone(),
            description: data.description.clone(),
            category: data.category.clone(),
            price: plain_money(data.rental_price),
            notes: data.notes.clone(),
            initial_total: String::new(),
        })
    }

    pub fn toggle_rental_item(&mut self, item_id: &str) -> Result<String, String> {
        let id = parse_id(item_id)?;
        let mut rentals = Rentals::new(&mut self.db, &self.clock);
        let current = rentals.get_item(id).map_err(msg)?;
        let active = !current.is_active();
        rentals.set_item_active(id, active).map_err(msg)?;
        let text = if active {
            "Item ativado."
        } else {
            "Item desativado."
        };
        Ok(text.to_owned())
    }

    /// Adiciona unidades (`add = true`) ou dá baixa em unidades (`add = false`).
    pub fn change_rental_units(
        &mut self,
        item_id: &str,
        units: &str,
        reason: &str,
        add: bool,
    ) -> Result<String, String> {
        let id = parse_id(item_id)?;
        let quantity = parse_int("quantidade", units)?;
        let mut rentals = Rentals::new(&mut self.db, &self.clock);
        if add {
            rentals.add_units(id, quantity, reason).map_err(msg)?;
            Ok("Unidades adicionadas ao estoque de locação.".to_owned())
        } else {
            rentals.write_off(id, quantity, reason).map_err(msg)?;
            Ok("Baixa registrada.".to_owned())
        }
    }

    // ----- Locações -----

    pub fn rental_rows(&mut self) -> Result<Vec<RentalRow>, String> {
        let rentals = Rentals::new(&mut self.db, &self.clock)
            .list(&RentalFilter::default())
            .map_err(msg)?;
        let mut rows = Vec::with_capacity(rentals.len());
        for rental in rentals.iter().rev() {
            let summary = Rentals::new(&mut self.db, &self.clock)
                .summary(rental.id())
                .map_err(msg)?;
            let period = rental.period();
            rows.push(RentalRow {
                id: rental.id().to_string().into(),
                number: rental.number().into(),
                customer: self.customer_name(rental.customer_id()).into(),
                status: rental_status_text(rental.status(), summary.late).into(),
                period: format!("{} → {}", fmt_dt(period.start()), fmt_dt(period.end())).into(),
                total: summary.total.to_string().into(),
                balance: summary.balance.to_string().into(),
            });
        }
        Ok(rows)
    }

    /// Valida uma linha do rascunho e devolve a linha pronta para exibição.
    pub fn rental_draft_line(
        &mut self,
        item_index: i32,
        quantity: &str,
        price: &str,
    ) -> Result<DraftLineRow, String> {
        let items = self.active_rental_items()?;
        let item = usize::try_from(item_index)
            .ok()
            .and_then(|position| items.get(position))
            .ok_or_else(|| "selecione o item de locação".to_owned())?;

        let quantity_value = parse_int("quantidade", quantity)?;
        if quantity_value <= 0 {
            return Err("a quantidade deve ser maior que zero".to_owned());
        }
        let unit_price = match price.trim() {
            "" => item.data().rental_price,
            _ => parse_money("valor unitário", price)?,
        };
        let units = u32::try_from(quantity_value).map_err(msg)?;
        let total = unit_price.checked_mul_qty(units).map_err(msg)?;

        Ok(DraftLineRow {
            index: 0,
            item_id: item.id().to_string().into(),
            item: item.data().name.as_str().into(),
            quantity: quantity_value.to_string().into(),
            price: unit_price.to_string().into(),
            total: total.to_string().into(),
        })
    }

    pub fn create_rental(&mut self, form: RentalForm, lines: &[DraftLine]) -> Result<String, String> {
        let customer_id = self
            .customer_at(form.customer_index)
            .ok_or_else(|| "selecione o cliente".to_owned())?;
        let pickup = parse_datetime("retirada", &form.pickup)?;
        let expected = parse_datetime("devolução prevista", &form.expected)?;
        let discount = match form.discount.trim() {
            "" => Money::ZERO,
            _ => parse_money("desconto da locação", &form.discount)?,
        };
        let inputs = lines
            .iter()
            .map(rental_line_input)
            .collect::<Result<Vec<_>, String>>()?;

        let rental = Rentals::new(&mut self.db, &self.clock)
            .create(customer_id, pickup, expected, discount, inputs)
            .map_err(msg)?;
        Ok(format!("Locação {} criada.", rental.number()))
    }

    pub fn load_rental(&mut self, rental_id: &str) -> Result<RentalView, String> {
        let id = parse_id(rental_id)?;
        let summary = Rentals::new(&mut self.db, &self.clock)
            .summary(id)
            .map_err(msg)?;
        let items = self.all_rental_items()?;
        let rental = &summary.rental;

        let lines: Vec<RentalLineRow> = rental
            .lines()
            .iter()
            .enumerate()
            .map(|(index, line)| RentalLineRow {
                index: index as i32,
                item: item_name(&items, line.rental_item_id()).into(),
                quantity: line.quantity().to_string().into(),
                returned: line.returned().to_string().into(),
                damaged: line.damaged().to_string().into(),
                lost: line.lost().to_string().into(),
                outstanding: line.outstanding().to_string().into(),
            })
            .collect();

        let line_options: Vec<SharedString> = lines
            .iter()
            .map(|line| {
                SharedString::from(format!("{} — pendentes {}", line.item, line.outstanding))
            })
            .collect();

        let period = rental.period();
        let status = rental_status_text(rental.status(), summary.late);
        let summary_text = format!(
            "Situação: {} | Período: {} a {} | Total: {} | Recebido: {} | Saldo: {} | Pagamento: {} | Unidades em aberto: {}",
            status,
            fmt_dt(period.start()),
            fmt_dt(period.end()),
            summary.total,
            summary.received,
            summary.balance,
            payment_label(summary.payment_status),
            summary.outstanding_units,
        );

        Ok(RentalView {
            title: format!(
                "Locação {} — {}",
                rental.number(),
                self.customer_name(rental.customer_id())
            ),
            summary: summary_text,
            lines,
            line_options,
        })
    }

    pub fn rental_pick_up(&mut self, rental_id: &str) -> Result<String, String> {
        let id = parse_id(rental_id)?;
        Rentals::new(&mut self.db, &self.clock)
            .pick_up(id)
            .map_err(msg)?;
        Ok("Locação retirada.".to_owned())
    }

    pub fn rental_cancel(&mut self, rental_id: &str, reason: &str) -> Result<String, String> {
        let id = parse_id(rental_id)?;
        Rentals::new(&mut self.db, &self.clock)
            .cancel(id, reason)
            .map_err(msg)?;
        Ok("Locação cancelada.".to_owned())
    }

    pub fn rental_register_return(
        &mut self,
        rental_id: &str,
        line_index: i32,
        returned: &str,
        damaged: &str,
        lost: &str,
        notes: &str,
    ) -> Result<String, String> {
        let id = parse_id(rental_id)?;
        let rental = Rentals::new(&mut self.db, &self.clock)
            .get(id)
            .map_err(msg)?;
        let position = usize::try_from(line_index)
            .map_err(|_| "selecione a linha da locação".to_owned())?;
        let line = rental
            .lines()
            .get(position)
            .ok_or_else(|| "selecione a linha da locação".to_owned())?;

        let input = ReturnLineInput {
            line_id: line.id(),
            returned: parse_int("devolvidas", returned)?,
            damaged: parse_int("danificadas", damaged)?,
            lost: parse_int("perdidas", lost)?,
            notes: notes.to_owned(),
        };
        Rentals::new(&mut self.db, &self.clock)
            .register_return(id, vec![input])
            .map_err(msg)?;
        Ok("Devolução registrada.".to_owned())
    }

    pub fn rental_pay(
        &mut self,
        rental_id: &str,
        method_index: i32,
        amount: &str,
        notes: &str,
    ) -> Result<String, String> {
        let id = parse_id(rental_id)?;
        let value = parse_money("valor do pagamento", amount)?;
        Rentals::new(&mut self.db, &self.clock)
            .register_payment(id, payment_method_from_index(method_index), value, notes)
            .map_err(msg)?;
        Ok("Pagamento registrado.".to_owned())
    }

    // ----- Entregadores -----

    fn active_deliverers(&mut self) -> Result<Vec<Deliverer>, String> {
        Deliveries::new(&mut self.db, &self.clock)
            .list_deliverers(true)
            .map_err(msg)
    }

    pub fn deliverer_rows(&mut self) -> Result<Vec<DelivererRow>, String> {
        let deliverers = Deliveries::new(&mut self.db, &self.clock)
            .list_deliverers(false)
            .map_err(msg)?;
        Ok(deliverers.iter().map(deliverer_row).collect())
    }

    pub fn deliverer_options(&mut self) -> Result<Vec<SharedString>, String> {
        Ok(self
            .active_deliverers()?
            .iter()
            .map(|deliverer| SharedString::from(deliverer.data().name.as_str()))
            .collect())
    }

    pub fn load_deliverer(&mut self, deliverer_id: &str) -> Result<DelivererForm, String> {
        let id = parse_id(deliverer_id)?;
        let deliverer = Deliveries::new(&mut self.db, &self.clock)
            .get_deliverer(id)
            .map_err(msg)?;
        let data = deliverer.data();
        Ok(DelivererForm {
            id: deliverer.id().to_string(),
            name: data.name.clone(),
            phone: data.phone.clone(),
            document: data.document.clone().unwrap_or_default(),
            notes: data.notes.clone(),
        })
    }

    pub fn save_deliverer(&mut self, form: DelivererForm) -> Result<String, String> {
        let data = DelivererData {
            name: form.name,
            phone: form.phone,
            document: non_empty(&form.document),
            notes: form.notes,
        };
        if form.id.trim().is_empty() {
            Deliveries::new(&mut self.db, &self.clock)
                .register_deliverer(data)
                .map_err(msg)?;
            Ok("Entregador cadastrado.".to_owned())
        } else {
            let id = parse_id(&form.id)?;
            Deliveries::new(&mut self.db, &self.clock)
                .update_deliverer(id, data)
                .map_err(msg)?;
            Ok("Entregador atualizado.".to_owned())
        }
    }

    pub fn toggle_deliverer(&mut self, deliverer_id: &str) -> Result<String, String> {
        let id = parse_id(deliverer_id)?;
        let mut deliveries = Deliveries::new(&mut self.db, &self.clock);
        let current = deliveries.get_deliverer(id).map_err(msg)?;
        let active = !current.is_active();
        deliveries.set_deliverer_active(id, active).map_err(msg)?;
        let text = if active {
            "Entregador ativado."
        } else {
            "Entregador desativado."
        };
        Ok(text.to_owned())
    }

    // ----- Entregas -----

    /// Pedidos confirmados com entrega, com venda ativa e sem entrega registrada.
    fn pending_delivery_orders(&mut self) -> Result<Vec<Order>, String> {
        let orders = Orders::new(&mut self.db, &self.clock)
            .list(&OrderFilter {
                status: Some(OrderStatus::Confirmed),
                ..OrderFilter::default()
            })
            .map_err(msg)?;
        let sales = Sales::new(&mut self.db, &self.clock)
            .list(&SaleFilter::default())
            .map_err(msg)?;
        let sold: HashSet<Uuid> = sales
            .iter()
            .filter(|sale| sale.status() != SaleStatus::Cancelled)
            .map(|sale| sale.order_id())
            .collect();
        let deliveries = Deliveries::new(&mut self.db, &self.clock)
            .search(&DeliveryFilter::default())
            .map_err(msg)?;
        let scheduled: HashSet<Uuid> = deliveries.iter().map(|delivery| delivery.order_id()).collect();

        Ok(orders
            .into_iter()
            .filter(|order| {
                order.receive_mode() == ReceiveMode::Delivery
                    && sold.contains(&order.id())
                    && !scheduled.contains(&order.id())
            })
            .collect())
    }

    pub fn delivery_order_options(&mut self) -> Result<Vec<SharedString>, String> {
        let orders = self.pending_delivery_orders()?;
        let mut options = Vec::with_capacity(orders.len());
        for order in &orders {
            let total = order.total().map_err(msg)?;
            options.push(SharedString::from(format!(
                "{} — {}",
                self.customer_name(order.customer_id()),
                total
            )));
        }
        Ok(options)
    }

    fn customer_contact(&self, customer_id: Uuid) -> Option<String> {
        self.customers
            .iter()
            .find(|customer| customer.id() == customer_id)
            .and_then(|customer| {
                let data = customer.data();
                data.phone.clone().or_else(|| data.whatsapp.clone())
            })
    }

    pub fn create_delivery(&mut self, form: NewDeliveryForm) -> Result<String, String> {
        let orders = self.pending_delivery_orders()?;
        let order = usize::try_from(form.order_index)
            .ok()
            .and_then(|position| orders.get(position))
            .ok_or_else(|| "selecione o pedido".to_owned())?;

        let phone = non_empty(&form.phone)
            .or_else(|| self.customer_contact(order.customer_id()))
            .ok_or_else(|| "informe o telefone de contato".to_owned())?;
        let fee = match form.fee.trim() {
            "" => Money::ZERO,
            _ => parse_money("valor da entrega", &form.fee)?,
        };
        let scheduled_for = non_empty(&form.scheduled)
            .map(|text| parse_datetime("agendamento", &text))
            .transpose()?;

        Deliveries::new(&mut self.db, &self.clock)
            .create_for_order(
                order.id(),
                NewDeliveryInput {
                    contact_phone: phone,
                    fee,
                    notes: form.notes,
                    scheduled_for,
                },
            )
            .map_err(msg)?;
        Ok("Entrega criada.".to_owned())
    }

    /// `view_index`: 0 = lista geral, 1 = agenda diária, 2 = agenda semanal.
    /// Data vazia significa hoje.
    pub fn delivery_rows(&mut self, view_index: i32, date: &str) -> Result<Vec<DeliveryRow>, String> {
        let day = if date.trim().is_empty() {
            self.now().date()
        } else {
            parse_date("data", date)?
        };

        let result = match view_index {
            1 => Deliveries::new(&mut self.db, &self.clock).daily_agenda(day),
            2 => Deliveries::new(&mut self.db, &self.clock).weekly_agenda(day),
            _ => Deliveries::new(&mut self.db, &self.clock).search(&DeliveryFilter::default()),
        };
        let deliveries = result.map_err(msg)?;

        let deliverers = Deliveries::new(&mut self.db, &self.clock)
            .list_deliverers(false)
            .map_err(msg)?;

        let mut rows = Vec::with_capacity(deliveries.len());
        for delivery in &deliveries {
            let history = Deliveries::new(&mut self.db, &self.clock)
                .history(delivery.id())
                .map_err(msg)?;
            let deliverer = match history.assignments.iter().find(|a| a.is_active()) {
                Some(assignment) => deliverer_name(&deliverers, assignment.deliverer_id()),
                None if delivery.is_closed() => "—".to_owned(),
                None => "aguardando entregador".to_owned(),
            };
            rows.push(DeliveryRow {
                id: delivery.id().to_string().into(),
                schedule: delivery
                    .scheduled_for()
                    .map(fmt_dt)
                    .unwrap_or_else(|| "sem data".to_owned())
                    .into(),
                customer: self.customer_name(delivery.customer_id()).into(),
                status: delivery_status_text(delivery.status()).into(),
                deliverer: deliverer.into(),
                fee: delivery.fee().to_string().into(),
            });
        }
        Ok(rows)
    }

    pub fn delivery_panel_cards(&mut self) -> Result<Vec<DashCard>, String> {
        let panel = Deliveries::new(&mut self.db, &self.clock)
            .panel()
            .map_err(msg)?;
        Ok(vec![
            card("Entregas do dia", panel.today),
            card("Próximas entregas (2 h)", panel.upcoming),
            card("Entregas atrasadas", panel.overdue),
            card("Aguardando entregador", panel.awaiting_deliverer),
            card("Atribuídas", panel.assigned),
            card("Em andamento", panel.in_progress),
            card("Concluídas", panel.completed),
            card("Não realizadas", panel.not_delivered),
            card("Canceladas", panel.cancelled),
        ])
    }

    pub fn load_delivery(&mut self, delivery_id: &str) -> Result<DeliveryView, String> {
        let id = parse_id(delivery_id)?;
        let delivery = Deliveries::new(&mut self.db, &self.clock)
            .get(id)
            .map_err(msg)?;
        let history = Deliveries::new(&mut self.db, &self.clock)
            .history(id)
            .map_err(msg)?;
        let deliverers = Deliveries::new(&mut self.db, &self.clock)
            .list_deliverers(false)
            .map_err(msg)?;

        let deliverer = match history.assignments.iter().find(|a| a.is_active()) {
            Some(assignment) => deliverer_name(&deliverers, assignment.deliverer_id()),
            None => "nenhum".to_owned(),
        };
        let destination = delivery.destination();
        let notes = if delivery.notes().is_empty() {
            "—"
        } else {
            delivery.notes()
        };

        let details = format!(
            "Cliente: {}\nEndereço: {}, {} — {}\nContato: {}\nTaxa: {}\nSituação: {}\nAgendada para: {}\nEntregador: {}\nRecebido por: {}\nMotivo da não entrega: {}\nMotivo do cancelamento: {}\nObservações: {}",
            self.customer_name(delivery.customer_id()),
            destination.street(),
            destination.neighborhood(),
            destination.city(),
            delivery.contact_phone(),
            delivery.fee(),
            delivery_status_text(delivery.status()),
            delivery
                .scheduled_for()
                .map(fmt_dt)
                .unwrap_or_else(|| "sem data".to_owned()),
            deliverer,
            delivery.received_by().unwrap_or("—"),
            delivery.failure_reason().unwrap_or("—"),
            delivery.cancelled_reason().unwrap_or("—"),
            notes,
        );

        let mut lines: Vec<SharedString> = Vec::new();
        for change in &history.status_changes {
            let from = change.from.map(delivery_status_text).unwrap_or("criada");
            let note = if change.note.is_empty() {
                String::new()
            } else {
                format!(" — {}", change.note)
            };
            lines.push(SharedString::from(format!(
                "{}: {} → {}{}",
                fmt_dt(change.changed_at),
                from,
                delivery_status_text(change.to),
                note
            )));
        }
        for schedule in &history.schedules {
            lines.push(SharedString::from(format!(
                "{}: agendada para {}",
                fmt_dt(schedule.recorded_at),
                fmt_dt(schedule.scheduled_for)
            )));
        }

        Ok(DeliveryView {
            title: format!("Entrega — {}", self.customer_name(delivery.customer_id())),
            details,
            history: lines,
        })
    }

    pub fn schedule_delivery(&mut self, delivery_id: &str, when: &str) -> Result<String, String> {
        let id = parse_id(delivery_id)?;
        let when = parse_datetime("agendamento", when)?;
        Deliveries::new(&mut self.db, &self.clock)
            .schedule(id, when)
            .map_err(msg)?;
        Ok("Entrega agendada.".to_owned())
    }

    pub fn assign_delivery(&mut self, delivery_id: &str, deliverer_index: i32) -> Result<String, String> {
        let id = parse_id(delivery_id)?;
        let deliverers = self.active_deliverers()?;
        let deliverer = usize::try_from(deliverer_index)
            .ok()
            .and_then(|position| deliverers.get(position))
            .ok_or_else(|| "selecione o entregador".to_owned())?;
        Deliveries::new(&mut self.db, &self.clock)
            .assign(id, deliverer.id())
            .map_err(msg)?;
        Ok("Entregador atribuído.".to_owned())
    }

    pub fn depart_delivery(&mut self, delivery_id: &str) -> Result<String, String> {
        let id = parse_id(delivery_id)?;
        Deliveries::new(&mut self.db, &self.clock)
            .depart(id)
            .map_err(msg)?;
        Ok("Entrega em rota.".to_owned())
    }

    pub fn complete_delivery(
        &mut self,
        delivery_id: &str,
        received_by: &str,
        notes: &str,
    ) -> Result<String, String> {
        let id = parse_id(delivery_id)?;
        Deliveries::new(&mut self.db, &self.clock)
            .complete(id, received_by, notes)
            .map_err(msg)?;
        Ok("Entrega concluída.".to_owned())
    }

    pub fn fail_delivery(&mut self, delivery_id: &str, reason: &str) -> Result<String, String> {
        let id = parse_id(delivery_id)?;
        Deliveries::new(&mut self.db, &self.clock)
            .fail(id, reason)
            .map_err(msg)?;
        Ok("Não entrega registrada.".to_owned())
    }

    pub fn cancel_delivery(&mut self, delivery_id: &str, reason: &str) -> Result<String, String> {
        let id = parse_id(delivery_id)?;
        Deliveries::new(&mut self.db, &self.clock)
            .cancel(id, reason)
            .map_err(msg)?;
        Ok("Entrega cancelada.".to_owned())
    }
}