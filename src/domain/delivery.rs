use chrono::{Duration, NaiveDate, NaiveDateTime};
use uuid::Uuid;

use super::contact::normalize_phone;
use super::customer::Address;
use super::errors::DomainError;
use super::validation::required_text;
use super::value_objects::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    AwaitingScheduling,
    Scheduled,
    OutForDelivery,
    Delivered,
    NotDelivered,
    Cancelled,
}

impl DeliveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryStatus::AwaitingScheduling => "awaiting_scheduling",
            DeliveryStatus::Scheduled => "scheduled",
            DeliveryStatus::OutForDelivery => "out_for_delivery",
            DeliveryStatus::Delivered => "delivered",
            DeliveryStatus::NotDelivered => "not_delivered",
            DeliveryStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "awaiting_scheduling" => Some(DeliveryStatus::AwaitingScheduling),
            "scheduled" => Some(DeliveryStatus::Scheduled),
            "out_for_delivery" => Some(DeliveryStatus::OutForDelivery),
            "delivered" => Some(DeliveryStatus::Delivered),
            "not_delivered" => Some(DeliveryStatus::NotDelivered),
            "cancelled" => Some(DeliveryStatus::Cancelled),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DeliveryStatus::AwaitingScheduling => "aguardando agendamento",
            DeliveryStatus::Scheduled => "agendada",
            DeliveryStatus::OutForDelivery => "saiu para entrega",
            DeliveryStatus::Delivered => "entregue",
            DeliveryStatus::NotDelivered => "não entregue",
            DeliveryStatus::Cancelled => "cancelada",
        }
    }

    /// Entregue e cancelada não voltam a ser alteradas.
    pub fn is_terminal(self) -> bool {
        matches!(self, DeliveryStatus::Delivered | DeliveryStatus::Cancelled)
    }

    /// Tabela de transições permitidas. Reagendamento é a transição para `Scheduled` a partir de `Scheduled` ou `NotDelivered`.
    pub fn can_transition_to(self, to: DeliveryStatus) -> bool {
        use DeliveryStatus::*;
        matches!(
            (self, to),
            (AwaitingScheduling, Scheduled | Cancelled)
                | (Scheduled, Scheduled | OutForDelivery | Cancelled)
                | (OutForDelivery, Delivered | NotDelivered)
                | (NotDelivered, Scheduled | Cancelled)
        )
    }
}

/// Mudança de status registrada no histórico. `from` é `None` apenas na criação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChange {
    pub delivery_id: Uuid,
    pub from: Option<DeliveryStatus>,
    pub to: DeliveryStatus,
    pub changed_at: NaiveDateTime,
    pub note: String,
}

impl StatusChange {
    pub fn opening(delivery_id: Uuid, now: NaiveDateTime) -> Self {
        Self {
            delivery_id,
            from: None,
            to: DeliveryStatus::AwaitingScheduling,
            changed_at: now,
            note: "entrega criada".to_owned(),
        }
    }
}

/// Cada agendamento ou reagendamento gera um registro. Nenhum é apagado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleEntry {
    pub delivery_id: Uuid,
    pub scheduled_for: NaiveDateTime,
    pub recorded_at: NaiveDateTime,
}

/// Vínculo entre entrega e entregador. Só a data de encerramento pode mudar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryAssignment {
    id: Uuid,
    delivery_id: Uuid,
    deliverer_id: Uuid,
    assigned_at: NaiveDateTime,
    unassigned_at: Option<NaiveDateTime>,
}

impl DeliveryAssignment {
    pub fn open(id: Uuid, delivery_id: Uuid, deliverer_id: Uuid, now: NaiveDateTime) -> Self {
        Self {
            id,
            delivery_id,
            deliverer_id,
            assigned_at: now,
            unassigned_at: None,
        }
    }

    pub fn restore(
        id: Uuid,
        delivery_id: Uuid,
        deliverer_id: Uuid,
        assigned_at: NaiveDateTime,
        unassigned_at: Option<NaiveDateTime>,
    ) -> Self {
        Self { id, delivery_id, deliverer_id, assigned_at, unassigned_at }
    }

    pub fn close(&mut self, now: NaiveDateTime) -> Result<(), DomainError> {
        if self.unassigned_at.is_some() {
            return Err(DomainError::AssignmentClosed);
        }
        self.unassigned_at = Some(now);
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.unassigned_at.is_none()
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn delivery_id(&self) -> Uuid {
        self.delivery_id
    }

    pub fn deliverer_id(&self) -> Uuid {
        self.deliverer_id
    }

    pub fn assigned_at(&self) -> NaiveDateTime {
        self.assigned_at
    }

    pub fn unassigned_at(&self) -> Option<NaiveDateTime> {
        self.unassigned_at
    }
}

/// Histórico completo de uma entrega, para consulta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryHistory {
    pub status_changes: Vec<StatusChange>,
    pub schedules: Vec<ScheduleEntry>,
    pub assignments: Vec<DeliveryAssignment>,
}

fn optional_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    id: Uuid,
    order_id: Uuid,
    customer_id: Uuid,
    status: DeliveryStatus,
    destination: Address,
    contact_phone: String,
    fee: Money,
    notes: String,
    scheduled_for: Option<NaiveDateTime>,
    completed_at: Option<NaiveDateTime>,
    received_by: Option<String>,
    failure_reason: Option<String>,
    cancelled_reason: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl Delivery {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: Uuid,
        order_id: Uuid,
        customer_id: Uuid,
        destination: Address,
        contact_phone: &str,
        fee: Money,
        notes: &str,
        now: NaiveDateTime,
    ) -> Result<Self, DomainError> {
        if fee.is_negative() {
            return Err(DomainError::NegativeValue("valor da entrega"));
        }
        Ok(Self {
            id,
            order_id,
            customer_id,
            status: DeliveryStatus::AwaitingScheduling,
            destination,
            contact_phone: normalize_phone(contact_phone)?,
            fee,
            notes: notes.trim().to_owned(),
            scheduled_for: None,
            completed_at: None,
            received_by: None,
            failure_reason: None,
            cancelled_reason: None,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: Uuid,
        order_id: Uuid,
        customer_id: Uuid,
        status: DeliveryStatus,
        destination: Address,
        contact_phone: String,
        fee: Money,
        notes: String,
        scheduled_for: Option<NaiveDateTime>,
        completed_at: Option<NaiveDateTime>,
        received_by: Option<String>,
        failure_reason: Option<String>,
        cancelled_reason: Option<String>,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self {
            id,
            order_id,
            customer_id,
            status,
            destination,
            contact_phone,
            fee,
            notes,
            scheduled_for,
            completed_at,
            received_by,
            failure_reason,
            cancelled_reason,
            created_at,
            updated_at,
        }
    }

    /// Agenda ou reagenda. Retorna a mudança de status, ou `None` quando já estava agendada.
    pub fn schedule(
        &mut self,
        when: NaiveDateTime,
        now: NaiveDateTime,
    ) -> Result<Option<StatusChange>, DomainError> {
        if when < now {
            return Err(DomainError::DeliveryDateInPast);
        }
        let previous = self.status;
        let change = if previous == DeliveryStatus::Scheduled {
            self.updated_at = now;
            None
        } else {
            let note = if previous == DeliveryStatus::NotDelivered {
                "reagendamento"
            } else {
                "agendamento"
            };
            Some(self.transition(DeliveryStatus::Scheduled, now, note)?)
        };
        self.scheduled_for = Some(when);
        Ok(change)
    }

    /// Sai para entrega. Exige entregador atribuído, informado pela camada de aplicação.
    pub fn depart(&mut self, has_active_assignment: bool, now: NaiveDateTime) -> Result<StatusChange, DomainError> {
        self.ensure_transition(DeliveryStatus::OutForDelivery)?;
        if !has_active_assignment {
            return Err(DomainError::DeliveryNotAssigned);
        }
        self.transition(DeliveryStatus::OutForDelivery, now, "saída para entrega")
    }

    pub fn complete(
        &mut self,
        received_by: &str,
        notes: &str,
        now: NaiveDateTime,
    ) -> Result<StatusChange, DomainError> {
        self.ensure_transition(DeliveryStatus::Delivered)?;
        let note = notes.trim();
        let change = self.transition(
            DeliveryStatus::Delivered,
            now,
            if note.is_empty() { "entrega concluída" } else { note },
        )?;
        self.received_by = optional_text(received_by);
        self.completed_at = Some(now);
        Ok(change)
    }

    pub fn fail(&mut self, reason: &str, now: NaiveDateTime) -> Result<StatusChange, DomainError> {
        self.ensure_transition(DeliveryStatus::NotDelivered)?;
        let reason = required_text("motivo da não entrega", reason)?;
        let change = self.transition(DeliveryStatus::NotDelivered, now, &reason)?;
        self.failure_reason = Some(reason);
        Ok(change)
    }

    pub fn cancel(&mut self, reason: &str, now: NaiveDateTime) -> Result<StatusChange, DomainError> {
        self.ensure_transition(DeliveryStatus::Cancelled)?;
        let reason = required_text("motivo do cancelamento", reason)?;
        let change = self.transition(DeliveryStatus::Cancelled, now, &reason)?;
        self.cancelled_reason = Some(reason);
        Ok(change)
    }

    fn ensure_transition(&self, to: DeliveryStatus) -> Result<(), DomainError> {
        if self.status.can_transition_to(to) {
            Ok(())
        } else {
            Err(DomainError::InvalidDeliveryTransition {
                from: self.status.label(),
                to: to.label(),
            })
        }
    }

    fn transition(
        &mut self,
        to: DeliveryStatus,
        now: NaiveDateTime,
        note: &str,
    ) -> Result<StatusChange, DomainError> {
        self.ensure_transition(to)?;
        let change = StatusChange {
            delivery_id: self.id,
            from: Some(self.status),
            to,
            changed_at: now,
            note: note.to_owned(),
        };
        self.status = to;
        self.updated_at = now;
        Ok(change)
    }

    pub fn is_closed(&self) -> bool {
        self.status.is_terminal()
    }

    pub fn is_due_on(&self, day: NaiveDate) -> bool {
        self.scheduled_for.map(|when| when.date() == day).unwrap_or(false)
    }

    /// Agendada ou em andamento, com horário já passado.
    pub fn is_overdue(&self, now: NaiveDateTime) -> bool {
        matches!(self.status, DeliveryStatus::Scheduled | DeliveryStatus::OutForDelivery)
            && self.scheduled_for.map(|when| when < now).unwrap_or(false)
    }

    /// Agendada para o intervalo `[agora, agora + janela]`.
    pub fn is_upcoming(&self, now: NaiveDateTime, window: Duration) -> bool {
        self.status == DeliveryStatus::Scheduled
            && self
                .scheduled_for
                .map(|when| when >= now && when <= now + window)
                .unwrap_or(false)
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn order_id(&self) -> Uuid {
        self.order_id
    }

    pub fn customer_id(&self) -> Uuid {
        self.customer_id
    }

    pub fn status(&self) -> DeliveryStatus {
        self.status
    }

    pub fn destination(&self) -> &Address {
        &self.destination
    }

    pub fn contact_phone(&self) -> &str {
        &self.contact_phone
    }

    pub fn fee(&self) -> Money {
        self.fee
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }

    pub fn scheduled_for(&self) -> Option<NaiveDateTime> {
        self.scheduled_for
    }

    pub fn completed_at(&self) -> Option<NaiveDateTime> {
        self.completed_at
    }

    pub fn received_by(&self) -> Option<&str> {
        self.received_by.as_deref()
    }

    pub fn failure_reason(&self) -> Option<&str> {
        self.failure_reason.as_deref()
    }

    pub fn cancelled_reason(&self) -> Option<&str> {
        self.cancelled_reason.as_deref()
    }

    pub fn created_at(&self) -> NaiveDateTime {
        self.created_at
    }

    pub fn updated_at(&self) -> NaiveDateTime {
        self.updated_at
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn dt(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .and_then(|date| date.and_hms_opt(hour, 0, 0))
            .expect("data de teste válida")
    }

    fn delivery() -> Result<Delivery, DomainError> {
        Delivery::create(
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Address::new("Rua A, 1", "Centro", "Fortaleza")?,
            "11912345678",
            Money::from_cents(800),
            "",
            dt(16, 9),
        )
    }

    #[test]
    fn terminal_statuses_cannot_transition() {
        assert!(!DeliveryStatus::Delivered.can_transition_to(DeliveryStatus::Scheduled));
        assert!(!DeliveryStatus::Cancelled.can_transition_to(DeliveryStatus::Scheduled));
        assert!(DeliveryStatus::NotDelivered.can_transition_to(DeliveryStatus::Scheduled));
    }

    #[test]
    fn cannot_schedule_in_the_past() -> Result<(), DomainError> {
        let mut delivery = delivery()?;
        assert_eq!(
            delivery.schedule(dt(15, 9), dt(16, 9)),
            Err(DomainError::DeliveryDateInPast)
        );
        Ok(())
    }

    #[test]
    fn failure_requires_reason() -> Result<(), DomainError> {
        let mut delivery = delivery()?;
        delivery.schedule(dt(17, 9), dt(16, 9))?;
        delivery.depart(true, dt(17, 8))?;
        assert_eq!(
            delivery.fail("   ", dt(17, 10)),
            Err(DomainError::EmptyField("motivo da não entrega"))
        );
        Ok(())
    }

    #[test]
    fn rescheduling_a_scheduled_delivery_records_no_status_change() -> Result<(), DomainError> {
        let mut delivery = delivery()?;
        assert!(delivery.schedule(dt(17, 9), dt(16, 9))?.is_some());
        assert!(delivery.schedule(dt(18, 9), dt(16, 10))?.is_none());
        assert_eq!(delivery.scheduled_for(), Some(dt(18, 9)));
        Ok(())
    }
}