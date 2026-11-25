use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::stock::{self, PlannedMovement, StockOperation};
use super::validation::required_text;
use super::value_objects::Money;

/// Dados editáveis de um produto. Não inclui o saldo: o estoque só muda pelas movimentações.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductData {
    pub sku: String,
    pub name: String,
    pub description: String,
    pub category_id: Option<Uuid>,
    pub purchase_price: Money,
    pub sale_price: Money,
    pub min_stock: i64,
    pub unit: String,
}

impl ProductData {
    /// Valida os dados e devolve a versão normalizada (SKU em maiúsculas, textos sem espaços extras).
    pub fn validated(self) -> Result<Self, DomainError> {
        if self.purchase_price.is_negative() {
            return Err(DomainError::NegativeValue("preço de compra"));
        }
        if self.sale_price.is_negative() {
            return Err(DomainError::NegativeValue("preço de venda"));
        }
        if self.min_stock < 0 {
            return Err(DomainError::NegativeValue("estoque mínimo"));
        }
        Ok(Self {
            sku: required_text("SKU", &self.sku)?.to_uppercase(),
            name: required_text("nome", &self.name)?,
            description: self.description.trim().to_owned(),
            category_id: self.category_id,
            purchase_price: self.purchase_price,
            sale_price: self.sale_price,
            min_stock: self.min_stock,
            unit: required_text("unidade de medida", &self.unit)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    id: Uuid,
    data: ProductData,
    stock: i64,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl Product {
    /// Novo produto começa com saldo zero e ativo.
    pub fn create(id: Uuid, data: ProductData, now: NaiveDateTime) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            data: data.validated()?,
            stock: 0,
            active: true,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn restore(
        id: Uuid,
        data: ProductData,
        stock: i64,
        active: bool,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self { id, data, stock, active, created_at, updated_at }
    }

    /// Atualiza os dados editáveis. O saldo permanece inalterado.
    pub fn update(&mut self, data: ProductData, now: NaiveDateTime) -> Result<(), DomainError> {
        self.data = data.validated()?;
        self.updated_at = now;
        Ok(())
    }

    /// Exclusão lógica: o produto some das buscas padrão, mas o histórico permanece.
    pub fn set_active(&mut self, active: bool, now: NaiveDateTime) {
        self.active = active;
        self.updated_at = now;
    }

    /// Calcula a movimentação sem aplicá-la. Produto inativo não aceita movimentações.
    pub fn plan_stock_operation(
        &self,
        operation: StockOperation,
    ) -> Result<PlannedMovement, DomainError> {
        if !self.active {
            return Err(DomainError::ProductInactive);
        }
        stock::plan(self.stock, operation)
    }

    /// Estoque igual ou abaixo do mínimo (inclui itens zerados).
    pub fn is_low_stock(&self) -> bool {
        self.stock <= self.data.min_stock
    }

    pub fn is_out_of_stock(&self) -> bool {
        self.stock == 0
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn data(&self) -> &ProductData {
        &self.data
    }

    pub fn stock(&self) -> i64 {
        self.stock
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

    fn data(sku: &str, purchase_cents: i64, sale_cents: i64) -> ProductData {
        ProductData {
            sku: sku.to_owned(),
            name: "Mesa dobrável".to_owned(),
            description: "  Madeira  ".to_owned(),
            category_id: None,
            purchase_price: Money::from_cents(purchase_cents),
            sale_price: Money::from_cents(sale_cents),
            min_stock: 1,
            unit: "un".to_owned(),
        }
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("data de teste válida")
    }

    #[test]
    fn sku_is_trimmed_and_uppercased() -> Result<(), DomainError> {
        let normalized = data("  mes-01 ", 100, 200).validated()?;
        assert_eq!(normalized.sku, "MES-01");
        assert_eq!(normalized.description, "Madeira");
        Ok(())
    }

    #[test]
    fn negative_prices_are_rejected() {
        let result = data("MES-01", -1, 200).validated();
        assert_eq!(result, Err(DomainError::NegativeValue("preço de compra")));
    }

    #[test]
    fn inactive_product_refuses_stock_operations() {
        let mut product = Product::create(Uuid::now_v7(), data("MES-02", 100, 200), now())
            .expect("produto válido");
        product.set_active(false, now());

        let result = product.plan_stock_operation(StockOperation::Entry { quantity: 1 });
        assert_eq!(result, Err(DomainError::ProductInactive));
    }
}