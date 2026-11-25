use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::value_objects::Money;

/// Situação financeira, calculada a partir do total e do valor recebido. Não é armazenada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatus {
    Pending,
    Partial,
    Paid,
}

pub fn status_for(total: Money, received: Money) -> PaymentStatus {
    if received == Money::ZERO {
        PaymentStatus::Pending
    } else if received < total {
        PaymentStatus::Partial
    } else {
        PaymentStatus::Paid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMethod {
    Cash,
    Pix,
    CreditCard,
    DebitCard,
}

impl PaymentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentMethod::Cash => "cash",
            PaymentMethod::Pix => "pix",
            PaymentMethod::CreditCard => "credit_card",
            PaymentMethod::DebitCard => "debit_card",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "cash" => Some(PaymentMethod::Cash),
            "pix" => Some(PaymentMethod::Pix),
            "credit_card" => Some(PaymentMethod::CreditCard),
            "debit_card" => Some(PaymentMethod::DebitCard),
            _ => None,
        }
    }
}

/// A quem o pagamento se refere: uma venda ou uma locação, nunca as duas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentTarget {
    Sale(Uuid),
    Rental(Uuid),
}

/// Registro de dinheiro recebido. Imutável após gravado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    id: Uuid,
    target: PaymentTarget,
    method: PaymentMethod,
    amount: Money,
    paid_at: NaiveDateTime,
    notes: String,
}

impl Payment {
    pub fn new(
        id: Uuid,
        sale_id: Uuid,
        method: PaymentMethod,
        amount: Money,
        paid_at: NaiveDateTime,
        notes: &str,
    ) -> Result<Self, DomainError> {
        Self::build(id, PaymentTarget::Sale(sale_id), method, amount, paid_at, notes)
    }

    pub fn for_rental(
        id: Uuid,
        rental_id: Uuid,
        method: PaymentMethod,
        amount: Money,
        paid_at: NaiveDateTime,
        notes: &str,
    ) -> Result<Self, DomainError> {
        Self::build(id, PaymentTarget::Rental(rental_id), method, amount, paid_at, notes)
    }

    fn build(
        id: Uuid,
        target: PaymentTarget,
        method: PaymentMethod,
        amount: Money,
        paid_at: NaiveDateTime,
        notes: &str,
    ) -> Result<Self, DomainError> {
        if amount <= Money::ZERO {
            return Err(DomainError::NonPositivePayment);
        }
        Ok(Self {
            id,
            target,
            method,
            amount,
            paid_at,
            notes: notes.trim().to_owned(),
        })
    }

    pub fn restore(
        id: Uuid,
        target: PaymentTarget,
        method: PaymentMethod,
        amount: Money,
        paid_at: NaiveDateTime,
        notes: String,
    ) -> Self {
        Self { id, target, method, amount, paid_at, notes }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn target(&self) -> PaymentTarget {
        self.target
    }

    pub fn method(&self) -> PaymentMethod {
        self.method
    }

    pub fn amount(&self) -> Money {
        self.amount
    }

    pub fn paid_at(&self) -> NaiveDateTime {
        self.paid_at
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("data de teste válida")
    }

    #[test]
    fn payment_amount_must_be_positive() {
        let result = Payment::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            PaymentMethod::Cash,
            Money::ZERO,
            now(),
            "",
        );
        assert_eq!(result, Err(DomainError::NonPositivePayment));
    }

    #[test]
    fn method_round_trips_through_storage_text() {
        for method in [
            PaymentMethod::Cash,
            PaymentMethod::Pix,
            PaymentMethod::CreditCard,
            PaymentMethod::DebitCard,
        ] {
            assert_eq!(PaymentMethod::parse(method.as_str()), Some(method));
        }
    }

    #[test]
    fn status_follows_received_amount() {
        let total = Money::from_cents(1000);
        assert_eq!(status_for(total, Money::ZERO), PaymentStatus::Pending);
        assert_eq!(status_for(total, Money::from_cents(500)), PaymentStatus::Partial);
        assert_eq!(status_for(total, total), PaymentStatus::Paid);
    }
}