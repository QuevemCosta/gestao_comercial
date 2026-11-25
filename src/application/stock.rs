use uuid::Uuid;

use crate::domain::product::Product;
use crate::domain::repositories::{ProductFilter, StockAlert};
use crate::domain::stock::{StockMovement, StockOperation};
use crate::domain::validation::required_text;

use super::clock::Clock;
use super::error::AppError;
use super::unit_of_work::Transactor;

/// Casos de uso de estoque. Toda alteração de saldo passa por aqui.
pub struct StockControl<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> StockControl<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> StockControl<'_, S> {
    pub fn register_entry(
        &mut self,
        product_id: Uuid,
        quantity: i64,
        reason: &str,
    ) -> Result<StockMovement, AppError> {
        self.apply(product_id, StockOperation::Entry { quantity }, reason)
    }

    pub fn register_exit(
        &mut self,
        product_id: Uuid,
        quantity: i64,
        reason: &str,
    ) -> Result<StockMovement, AppError> {
        self.apply(product_id, StockOperation::Exit { quantity }, reason)
    }

    /// Ajuste por contagem física: registra a diferença entre o saldo atual e o contado.
    pub fn adjust_stock(
        &mut self,
        product_id: Uuid,
        counted: i64,
        reason: &str,
    ) -> Result<StockMovement, AppError> {
        self.apply(product_id, StockOperation::AdjustTo { counted }, reason)
    }

    pub fn movements_for(&mut self, product_id: Uuid) -> Result<Vec<StockMovement>, AppError> {
        let uow = self.store.begin()?;
        let movements = uow.movements_for(product_id)?;
        uow.commit()?;
        Ok(movements)
    }

    /// Produtos ativos com saldo igual ou abaixo do mínimo.
    pub fn low_stock_products(&mut self) -> Result<Vec<Product>, AppError> {
        let filter = ProductFilter {
            alert: Some(StockAlert::LowStock),
            ..ProductFilter::default()
        };
        let uow = self.store.begin()?;
        let products = uow.search_products(&filter)?;
        uow.commit()?;
        Ok(products)
    }

    /// Leitura do produto, validação, cálculo e gravação ocorrem na mesma unidade de trabalho.
    fn apply(
        &mut self,
        product_id: Uuid,
        operation: StockOperation,
        reason: &str,
    ) -> Result<StockMovement, AppError> {
        let reason = required_text("motivo", reason)?;
        let now = self.clock.now();

        let mut uow = self.store.begin()?;
        let product = uow
            .find_product(product_id)?
            .ok_or(AppError::NotFound("produto"))?;

        let planned = product.plan_stock_operation(operation)?;
        let movement = StockMovement::new(Uuid::now_v7(), product_id, &planned, &reason, None, now)?;

        uow.record_stock_movement(&movement, product.stock())?;
        uow.commit()?;
        Ok(movement)
    }
}