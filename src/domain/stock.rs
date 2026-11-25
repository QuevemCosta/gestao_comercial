use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::validation::required_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    Entry,
    Exit,
    Adjustment,
    Reversal,
}

impl MovementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MovementKind::Entry => "entry",
            MovementKind::Exit => "exit",
            MovementKind::Adjustment => "adjustment",
            MovementKind::Reversal => "reversal",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "entry" => Some(MovementKind::Entry),
            "exit" => Some(MovementKind::Exit),
            "adjustment" => Some(MovementKind::Adjustment),
            "reversal" => Some(MovementKind::Reversal),
            _ => None,
        }
    }
}

/// Pedidos de alteração de saldo feitos pelo usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockOperation {
    Entry { quantity: i64 },
    Exit { quantity: i64 },
    /// Contagem física: o saldo passa a ser `counted`.
    AdjustTo { counted: i64 },
}

/// Resultado do cálculo, ainda sem persistência.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMovement {
    pub kind: MovementKind,
    pub delta: i64,
    pub balance_after: i64,
}

/// Calcula a movimentação a partir do saldo atual. Função pura, sem acesso a banco.
pub fn plan(current: i64, operation: StockOperation) -> Result<PlannedMovement, DomainError> {
    match operation {
        StockOperation::Entry { quantity } => {
            ensure_positive(quantity)?;
            let balance_after = current.checked_add(quantity).ok_or(DomainError::Overflow)?;
            Ok(PlannedMovement {
                kind: MovementKind::Entry,
                delta: quantity,
                balance_after,
            })
        }
        StockOperation::Exit { quantity } => {
            ensure_positive(quantity)?;
            if quantity > current {
                return Err(DomainError::InsufficientStock {
                    available: current,
                    requested: quantity,
                });
            }
            Ok(PlannedMovement {
                kind: MovementKind::Exit,
                delta: -quantity,
                balance_after: current - quantity,
            })
        }
        StockOperation::AdjustTo { counted } => {
            if counted < 0 {
                return Err(DomainError::NegativeCount);
            }
            let delta = counted.checked_sub(current).ok_or(DomainError::Overflow)?;
            if delta == 0 {
                return Err(DomainError::NoStockChange);
            }
            Ok(PlannedMovement {
                kind: MovementKind::Adjustment,
                delta,
                balance_after: counted,
            })
        }
    }
}

fn ensure_positive(quantity: i64) -> Result<(), DomainError> {
    if quantity > 0 {
        Ok(())
    } else {
        Err(DomainError::InvalidQuantity)
    }
}

/// Registro imutável de uma movimentação já aplicada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockMovement {
    id: Uuid,
    product_id: Uuid,
    kind: MovementKind,
    delta: i64,
    balance_after: i64,
    reason: String,
    reverses: Option<Uuid>,
    occurred_at: NaiveDateTime,
}

impl StockMovement {
    pub fn new(
        id: Uuid,
        product_id: Uuid,
        planned: &PlannedMovement,
        reason: &str,
        reverses: Option<Uuid>,
        occurred_at: NaiveDateTime,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            product_id,
            kind: planned.kind,
            delta: planned.delta,
            balance_after: planned.balance_after,
            reason: required_text("motivo", reason)?,
            reverses,
            occurred_at,
        })
    }

    pub fn restore(
        id: Uuid,
        product_id: Uuid,
        planned: PlannedMovement,
        reason: String,
        reverses: Option<Uuid>,
        occurred_at: NaiveDateTime,
    ) -> Self {
        Self {
            id,
            product_id,
            kind: planned.kind,
            delta: planned.delta,
            balance_after: planned.balance_after,
            reason,
            reverses,
            occurred_at,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn product_id(&self) -> Uuid {
        self.product_id
    }

    pub fn kind(&self) -> MovementKind {
        self.kind
    }

    pub fn delta(&self) -> i64 {
        self.delta
    }

    pub fn balance_after(&self) -> i64 {
        self.balance_after
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn reverses(&self) -> Option<Uuid> {
        self.reverses
    }

    pub fn occurred_at(&self) -> NaiveDateTime {
        self.occurred_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_increases_balance() -> Result<(), DomainError> {
        let planned = plan(3, StockOperation::Entry { quantity: 4 })?;
        assert_eq!(planned.kind, MovementKind::Entry);
        assert_eq!(planned.delta, 4);
        assert_eq!(planned.balance_after, 7);
        Ok(())
    }

    #[test]
    fn exit_cannot_exceed_balance() {
        let result = plan(2, StockOperation::Exit { quantity: 3 });
        assert_eq!(
            result,
            Err(DomainError::InsufficientStock { available: 2, requested: 3 })
        );
    }

    #[test]
    fn exit_produces_negative_delta() -> Result<(), DomainError> {
        let planned = plan(10, StockOperation::Exit { quantity: 4 })?;
        assert_eq!(planned.delta, -4);
        assert_eq!(planned.balance_after, 6);
        Ok(())
    }

    #[test]
    fn adjustment_records_difference() -> Result<(), DomainError> {
        let planned = plan(5, StockOperation::AdjustTo { counted: 3 })?;
        assert_eq!(planned.kind, MovementKind::Adjustment);
        assert_eq!(planned.delta, -2);
        assert_eq!(planned.balance_after, 3);
        Ok(())
    }

    #[test]
    fn adjustment_without_difference_is_rejected() {
        let result = plan(5, StockOperation::AdjustTo { counted: 5 });
        assert_eq!(result, Err(DomainError::NoStockChange));
    }

    #[test]
    fn zero_quantity_is_rejected() {
        let result = plan(5, StockOperation::Entry { quantity: 0 });
        assert_eq!(result, Err(DomainError::InvalidQuantity));
    }
}