use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::payment::PaymentStatus;
use super::stock::{MovementKind, PlannedMovement};
use super::validation::required_text;
use super::value_objects::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleStatus {
    Confirmed,
    Completed,
    Cancelled,
}

impl SaleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SaleStatus::Confirmed => "confirmed",
            SaleStatus::Completed => "completed",
            SaleStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "confirmed" => Some(SaleStatus::Confirmed),
            "completed" => Some(SaleStatus::Completed),
            "cancelled" => Some(SaleStatus::Cancelled),
            _ => None,
        }
    }
}

/// Nome da sequência de numeração de vendas de um ano.
pub fn sequence_name(year: i32) -> String {
    format!("sale-{year}")
}

/// Número legível da venda, por exemplo `V-2026-0007`.
pub fn format_number(year: i32, sequence: i64) -> String {
    format!("V-{year}-{sequence:04}")
}

/// Item da venda: cópia dos valores negociados e vínculo com o movimento de saída.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleItem {
    id: Uuid,
    product_id: Uuid,
    quantity: i64,
    unit_price: Money,
    discount: Money,
    stock_movement_id: Uuid,
}

impl SaleItem {
    pub fn new(
        id: Uuid,
        product_id: Uuid,
        quantity: i64,
        unit_price: Money,
        discount: Money,
        stock_movement_id: Uuid,
    ) -> Result<Self, DomainError> {
        if quantity <= 0 {
            return Err(DomainError::InvalidQuantity);
        }
        if unit_price.is_negative() {
            return Err(DomainError::NegativeValue("preço unitário"));
        }
        if discount.is_negative() {
            return Err(DomainError::NegativeValue("desconto do item"));
        }
        let item = Self { id, product_id, quantity, unit_price, discount, stock_movement_id };
        if item.discount > item.gross()? {
            return Err(DomainError::DiscountExceedsAmount);
        }
        Ok(item)
    }

    pub fn restore(
        id: Uuid,
        product_id: Uuid,
        quantity: i64,
        unit_price: Money,
        discount: Money,
        stock_movement_id: Uuid,
    ) -> Self {
        Self { id, product_id, quantity, unit_price, discount, stock_movement_id }
    }

    pub fn gross(&self) -> Result<Money, DomainError> {
        let quantity = u32::try_from(self.quantity).map_err(|_| DomainError::Overflow)?;
        self.unit_price.checked_mul_qty(quantity)
    }

    pub fn net(&self) -> Result<Money, DomainError> {
        self.gross()?.checked_sub(self.discount)
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn product_id(&self) -> Uuid {
        self.product_id
    }

    pub fn quantity(&self) -> i64 {
        self.quantity
    }

    pub fn unit_price(&self) -> Money {
        self.unit_price
    }

    pub fn discount(&self) -> Money {
        self.discount
    }

    pub fn stock_movement_id(&self) -> Uuid {
        self.stock_movement_id
    }
}

/// Venda confirmada. A situação financeira não é armazenada: é calculada a partir dos pagamentos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sale {
    id: Uuid,
    number: String,
    order_id: Uuid,
    customer_id: Uuid,
    status: SaleStatus,
    order_discount: Money,
    items: Vec<SaleItem>,
    cancelled_reason: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    completed_at: Option<NaiveDateTime>,
}

impl Sale {
    pub fn confirm(
        id: Uuid,
        number: String,
        order_id: Uuid,
        customer_id: Uuid,
        order_discount: Money,
        items: Vec<SaleItem>,
        now: NaiveDateTime,
    ) -> Result<Self, DomainError> {
        if items.is_empty() {
            return Err(DomainError::EmptySale);
        }
        if order_discount.is_negative() {
            return Err(DomainError::NegativeValue("desconto do pedido"));
        }
        if order_discount > items_net_total(&items)? {
            return Err(DomainError::DiscountExceedsAmount);
        }
        Ok(Self {
            id,
            number,
            order_id,
            customer_id,
            status: SaleStatus::Confirmed,
            order_discount,
            items,
            cancelled_reason: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: Uuid,
        number: String,
        order_id: Uuid,
        customer_id: Uuid,
        status: SaleStatus,
        order_discount: Money,
        items: Vec<SaleItem>,
        cancelled_reason: Option<String>,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
        completed_at: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            id,
            number,
            order_id,
            customer_id,
            status,
            order_discount,
            items,
            cancelled_reason,
            created_at,
            updated_at,
            completed_at,
        }
    }

    /// Marca a venda como concluída (cliente recebeu ou retirou). Não altera pagamentos.
    pub fn complete(&mut self, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_confirmed()?;
        self.status = SaleStatus::Completed;
        self.completed_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Cancela a venda. Exige motivo. O estorno de estoque é gerado pela camada de aplicação.
    pub fn cancel(&mut self, reason: &str, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_confirmed()?;
        let reason = required_text("motivo do cancelamento", reason)?;
        self.status = SaleStatus::Cancelled;
        self.cancelled_reason = Some(reason);
        self.updated_at = now;
        Ok(())
    }

    pub fn ensure_accepts_payments(&self) -> Result<(), DomainError> {
        if self.status == SaleStatus::Cancelled {
            Err(DomainError::SaleCancelled)
        } else {
            Ok(())
        }
    }

    pub fn subtotal(&self) -> Result<Money, DomainError> {
        self.items
            .iter()
            .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.gross()?))
    }

    pub fn items_discount(&self) -> Result<Money, DomainError> {
        self.items
            .iter()
            .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.discount()))
    }

    /// Total da venda: itens após descontos, menos o desconto do pedido.
    pub fn total(&self) -> Result<Money, DomainError> {
        items_net_total(&self.items)?.checked_sub(self.order_discount)
    }

    /// Situação financeira segundo o valor já recebido.
    pub fn payment_status(&self, received: Money) -> Result<PaymentStatus, DomainError> {
        let total = self.total()?;
        Ok(if received == Money::ZERO {
            PaymentStatus::Pending
        } else if received < total {
            PaymentStatus::Partial
        } else {
            PaymentStatus::Paid
        })
    }

    fn ensure_confirmed(&self) -> Result<(), DomainError> {
        if self.status == SaleStatus::Confirmed {
            Ok(())
        } else {
            Err(DomainError::SaleNotConfirmed)
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn number(&self) -> &str {
        &self.number
    }

    pub fn order_id(&self) -> Uuid {
        self.order_id
    }

    pub fn customer_id(&self) -> Uuid {
        self.customer_id
    }

    pub fn status(&self) -> SaleStatus {
        self.status
    }

    pub fn order_discount(&self) -> Money {
        self.order_discount
    }

    pub fn items(&self) -> &[SaleItem] {
        &self.items
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

    pub fn completed_at(&self) -> Option<NaiveDateTime> {
        self.completed_at
    }
}

/// Planeja o estorno de uma saída de estoque: devolve a quantidade ao saldo.
pub fn plan_reversal(current: i64, quantity: i64) -> Result<PlannedMovement, DomainError> {
    let balance_after = current.checked_add(quantity).ok_or(DomainError::Overflow)?;
    Ok(PlannedMovement {
        kind: MovementKind::Reversal,
        delta: quantity,
        balance_after,
    })
}

fn items_net_total(items: &[SaleItem]) -> Result<Money, DomainError> {
    items
        .iter()
        .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.net()?))
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

    fn item(quantity: i64, unit: i64) -> Result<SaleItem, DomainError> {
        SaleItem::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            quantity,
            Money::from_cents(unit),
            Money::ZERO,
            Uuid::now_v7(),
        )
    }

    fn confirmed(items: Vec<SaleItem>) -> Result<Sale, DomainError> {
        Sale::confirm(
            Uuid::now_v7(),
            format_number(2026, 1),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Money::ZERO,
            items,
            now(),
        )
    }

    #[test]
    fn numbers_are_padded_and_scoped_by_year() {
        assert_eq!(format_number(2026, 7), "V-2026-0007");
        assert_eq!(sequence_name(2026), "sale-2026");
    }

    #[test]
    fn sale_without_items_is_rejected() -> Result<(), DomainError> {
        assert_eq!(confirmed(Vec::new()).unwrap_err(), DomainError::EmptySale);
        let _ = item(1, 100)?;
        Ok(())
    }

    #[test]
    fn payment_status_follows_received_amount() -> Result<(), DomainError> {
        let sale = confirmed(vec![item(2, 1000)?])?;
        assert_eq!(sale.payment_status(Money::ZERO)?, PaymentStatus::Pending);
        assert_eq!(sale.payment_status(Money::from_cents(500))?, PaymentStatus::Partial);
        assert_eq!(sale.payment_status(Money::from_cents(2000))?, PaymentStatus::Paid);
        Ok(())
    }

    #[test]
    fn only_confirmed_sales_can_be_completed_or_cancelled() -> Result<(), DomainError> {
        let mut sale = confirmed(vec![item(1, 100)?])?;
        sale.complete(now())?;
        assert_eq!(sale.cancel("motivo", now()), Err(DomainError::SaleNotConfirmed));
        Ok(())
    }

    #[test]
    fn cancellation_requires_reason() -> Result<(), DomainError> {
        let mut sale = confirmed(vec![item(1, 100)?])?;
        assert_eq!(
            sale.cancel("   ", now()),
            Err(DomainError::EmptyField("motivo do cancelamento"))
        );
        Ok(())
    }

    #[test]
    fn reversal_returns_quantity_to_balance() -> Result<(), DomainError> {
        let planned = plan_reversal(6, 4)?;
        assert_eq!(planned.kind, MovementKind::Reversal);
        assert_eq!(planned.delta, 4);
        assert_eq!(planned.balance_after, 10);
        Ok(())
    }
}