use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::validation::required_text;
use super::value_objects::Money;

/// Dados editáveis de um item de locação. A quantidade total é alterada somente
/// por `add_units` e `write_off`, que são auditados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalItemData {
    pub name: String,
    pub description: String,
    pub category: String,
    pub rental_price: Money,
    pub notes: String,
}

impl RentalItemData {
    pub fn validated(self) -> Result<Self, DomainError> {
        if self.rental_price.is_negative() {
            return Err(DomainError::NegativeValue("valor da locação"));
        }
        Ok(Self {
            name: required_text("nome", &self.name)?,
            description: self.description.trim().to_owned(),
            category: required_text("categoria", &self.category)?,
            rental_price: self.rental_price,
            notes: self.notes.trim().to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalItem {
    id: Uuid,
    data: RentalItemData,
    total_quantity: i64,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl RentalItem {
    pub fn create(
        id: Uuid,
        data: RentalItemData,
        initial_total: i64,
        now: NaiveDateTime,
    ) -> Result<Self, DomainError> {
        if initial_total < 0 {
            return Err(DomainError::NegativeValue("quantidade total"));
        }
        Ok(Self {
            id,
            data: data.validated()?,
            total_quantity: initial_total,
            active: true,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn restore(
        id: Uuid,
        data: RentalItemData,
        total_quantity: i64,
        active: bool,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self { id, data, total_quantity, active, created_at, updated_at }
    }

    pub fn update(&mut self, data: RentalItemData, now: NaiveDateTime) -> Result<(), DomainError> {
        self.data = data.validated()?;
        self.updated_at = now;
        Ok(())
    }

    /// Entrada de novas unidades, por exemplo após compra.
    pub fn add_units(&mut self, quantity: i64, now: NaiveDateTime) -> Result<(), DomainError> {
        if quantity <= 0 {
            return Err(DomainError::InvalidQuantity);
        }
        self.total_quantity = self
            .total_quantity
            .checked_add(quantity)
            .ok_or(DomainError::Overflow)?;
        self.updated_at = now;
        Ok(())
    }

    /// Baixa definitiva de unidades perdidas ou inutilizáveis.
    pub fn write_off(&mut self, quantity: i64, now: NaiveDateTime) -> Result<(), DomainError> {
        if quantity <= 0 {
            return Err(DomainError::InvalidQuantity);
        }
        if quantity > self.total_quantity {
            return Err(DomainError::WriteOffExceedsTotal { total: self.total_quantity });
        }
        self.total_quantity -= quantity;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_active(&mut self, active: bool, now: NaiveDateTime) {
        self.active = active;
        self.updated_at = now;
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn data(&self) -> &RentalItemData {
        &self.data
    }

    pub fn total_quantity(&self) -> i64 {
        self.total_quantity
    }

    pub fn is_active(&self) -> bool {
        self.active
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

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("data de teste válida")
    }

    fn data() -> RentalItemData {
        RentalItemData {
            name: "Cadeira".to_owned(),
            description: String::new(),
            category: "Mobiliário".to_owned(),
            rental_price: Money::from_cents(500),
            notes: String::new(),
        }
    }

    #[test]
    fn write_off_cannot_exceed_total() -> Result<(), DomainError> {
        let mut item = RentalItem::create(Uuid::now_v7(), data(), 5, now())?;
        assert_eq!(
            item.write_off(6, now()),
            Err(DomainError::WriteOffExceedsTotal { total: 5 })
        );
        item.write_off(2, now())?;
        assert_eq!(item.total_quantity(), 3);
        Ok(())
    }

    #[test]
    fn add_units_requires_positive_quantity() -> Result<(), DomainError> {
        let mut item = RentalItem::create(Uuid::now_v7(), data(), 0, now())?;
        assert_eq!(item.add_units(0, now()), Err(DomainError::InvalidQuantity));
        item.add_units(10, now())?;
        assert_eq!(item.total_quantity(), 10);
        Ok(())
    }
}