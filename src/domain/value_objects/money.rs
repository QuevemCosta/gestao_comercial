use std::fmt;

use crate::domain::errors::DomainError;

/// Valor monetário armazenado em centavos, para evitar erros de ponto flutuante.
///
/// Não implementa `Add` e `Sub` diretamente, porque o overflow silencioso em
/// valores financeiros é inaceitável. As operações retornam `Result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Money) -> Result<Money, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Money)
            .ok_or(DomainError::Overflow)
    }

    pub fn checked_sub(self, other: Money) -> Result<Money, DomainError> {
        self.0
            .checked_sub(other.0)
            .map(Money)
            .ok_or(DomainError::Overflow)
    }

    /// Multiplica o valor unitário pela quantidade.
    pub fn checked_mul_qty(self, quantity: u32) -> Result<Money, DomainError> {
        self.0
            .checked_mul(i64::from(quantity))
            .map(Money)
            .ok_or(DomainError::Overflow)
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl fmt::Display for Money {
    /// Formata no padrão brasileiro, por exemplo `R$ 1.234,56`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let absolute = self.0.unsigned_abs();
        let reais = absolute / 100;
        let centavos = absolute % 100;
        write!(f, "{sign}R$ {},{centavos:02}", group_thousands(reais))
    }
}

fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
                if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_cents_exactly() {
        assert_eq!(Money::from_cents(1999).cents(), 1999);
    }

    #[test]
    fn displays_in_brazilian_format() {
        assert_eq!(Money::from_cents(0).to_string(), "R$ 0,00");
        assert_eq!(Money::from_cents(5).to_string(), "R$ 0,05");
        assert_eq!(Money::from_cents(123_456_789).to_string(), "R$ 1.234.567,89");
    }

    #[test]
    fn displays_negative_values() {
        assert_eq!(Money::from_cents(-150).to_string(), "-R$ 1,50");
    }

    #[test]
    fn multiplies_by_quantity() -> Result<(), DomainError> {
        let unit = Money::from_cents(1250);
        assert_eq!(unit.checked_mul_qty(4)?, Money::from_cents(5000));
        Ok(())
    }

    #[test]
    fn detects_overflow_on_multiplication() {
        let huge = Money::from_cents(i64::MAX);
        assert_eq!(huge.checked_mul_qty(2), Err(DomainError::Overflow));
    }

    #[test]
    fn detects_overflow_on_addition() {
        let huge = Money::from_cents(i64::MAX);
        assert_eq!(huge.checked_add(Money::from_cents(1)), Err(DomainError::Overflow));
    }

    #[test]
    fn subtraction_can_produce_negative_values() -> Result<(), DomainError> {
        let result = Money::from_cents(100).checked_sub(Money::from_cents(250))?;
        assert!(result.is_negative());
        assert_eq!(result.cents(), -150);
        Ok(())
    }
}