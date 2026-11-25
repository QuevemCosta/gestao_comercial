use chrono::Datelike;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::payment::{Payment, PaymentMethod, PaymentStatus};
use crate::domain::repositories::SaleFilter;
use crate::domain::sale::{self, Sale, SaleItem};
use crate::domain::stock::{StockMovement, StockOperation};
use crate::domain::value_objects::Money;

use super::clock::Clock;
use super::deliveries;
use super::error::AppError;
use super::unit_of_work::{Transactor, UnitOfWork};

/// Visão consolidada de uma venda: total, valores recebidos e situação financeira.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleSummary {
    pub sale: Sale,
    pub total: Money,
    pub received: Money,
    pub balance: Money,
    pub payment_status: PaymentStatus,
    pub payments: Vec<Payment>,
}

/// Resultado do cancelamento. Se `received` for maior que zero, a interface deve
/// alertar o operador para tratamento manual do valor recebido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleCancellation {
    pub sale: Sale,
    pub received: Money,
}

/// Casos de uso de vendas e pagamentos.
pub struct Sales<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Sales<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Sales<'_, S> {
    /// Confirma o pedido: gera a venda, baixa o estoque e marca o pedido como confirmado.
    /// Tudo ocorre em uma única unidade de trabalho.
    pub fn confirm_order(&mut self, order_id: Uuid) -> Result<Sale, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = uow
            .find_order(order_id)?
            .ok_or(AppError::NotFound("pedido"))?;
        order.confirm(now)?;

        let year = now.year();
        let sequence = uow.next_sequence_value(&sale::sequence_name(year))?;
        let number = sale::format_number(year, sequence);
        let reason = format!("Venda {number}");

        let mut items = Vec::with_capacity(order.items().len());
        for line in order.items() {
            let product = uow
                .find_product(line.product_id())?
                .ok_or(AppError::NotFound("produto"))?;
            let planned =
                product.plan_stock_operation(StockOperation::Exit { quantity: line.quantity() })?;
            let movement =
                StockMovement::new(Uuid::now_v7(), line.product_id(), &planned, &reason, None, now)?;
            uow.record_stock_movement(&movement, product.stock())?;
            items.push(SaleItem::new(
                Uuid::now_v7(),
                line.product_id(),
                line.quantity(),
                line.unit_price(),
                line.discount(),
                movement.id(),
            )?);
        }

        let sale = Sale::confirm(
            Uuid::now_v7(),
            number,
            order.id(),
            order.customer_id(),
            order.order_discount(),
            items,
            now,
        )?;
        uow.insert_sale(&sale)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(sale)
    }

    pub fn complete(&mut self, sale_id: Uuid) -> Result<Sale, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut sale = require_sale(&*uow, sale_id)?;
        sale.complete(now)?;
        uow.update_sale(&sale)?;
        uow.commit()?;
        Ok(sale)
    }

    /// Cancela a venda, estorna o estoque e cancela a entrega aberta (se houver).
    /// Entrega em andamento ou já concluída bloqueia o cancelamento.
    /// O valor já recebido é devolvido no resultado para tratamento manual.
    pub fn cancel(&mut self, sale_id: Uuid, reason: &str) -> Result<SaleCancellation, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut sale = require_sale(&*uow, sale_id)?;
        let payments = uow.payments_for_sale(sale_id)?;
        let received = sum_received(&payments)?;

        sale.cancel(reason, now)?;

        let reversal_reason = format!("Estorno da venda {}", sale.number());
        for line in sale.items() {
            let product = uow
                .find_product(line.product_id())?
                .ok_or(AppError::NotFound("produto"))?;
            let planned = sale::plan_reversal(product.stock(), line.quantity())?;
            let movement = StockMovement::new(
                Uuid::now_v7(),
                line.product_id(),
                &planned,
                &reversal_reason,
                Some(line.stock_movement_id()),
                now,
            )?;
            uow.record_stock_movement(&movement, product.stock())?;
        }

        deliveries::cancel_for_order_cancellation(&mut *uow, sale.order_id(), now)?;

        uow.update_sale(&sale)?;
        uow.commit()?;
        Ok(SaleCancellation { sale, received })
    }

    /// Registra um pagamento. Não pode exceder o saldo pendente nem ser feito em venda cancelada.
    pub fn register_payment(
        &mut self,
        sale_id: Uuid,
        method: PaymentMethod,
        amount: Money,
        notes: &str,
    ) -> Result<Payment, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let sale = require_sale(&*uow, sale_id)?;
        sale.ensure_accepts_payments()?;

        let payment = Payment::new(Uuid::now_v7(), sale_id, method, amount, now, notes)?;

        let payments = uow.payments_for_sale(sale_id)?;
        let received = sum_received(&payments)?;
        let balance = sale.total()?.checked_sub(received)?;
        if amount > balance {
            return Err(DomainError::PaymentExceedsBalance { balance }.into());
        }

        uow.insert_payment(&payment)?;
        uow.commit()?;
        Ok(payment)
    }

    pub fn summary(&mut self, sale_id: Uuid) -> Result<SaleSummary, AppError> {
        let uow = self.store.begin()?;
        let sale = require_sale(&*uow, sale_id)?;
        let payments = uow.payments_for_sale(sale_id)?;
        uow.commit()?;

        let received = sum_received(&payments)?;
        let total = sale.total()?;
        let balance = total.checked_sub(received)?;
        let payment_status = sale.payment_status(received)?;

        Ok(SaleSummary {
            sale,
            total,
            received,
            balance,
            payment_status,
            payments,
        })
    }

    pub fn get(&mut self, sale_id: Uuid) -> Result<Sale, AppError> {
        let uow = self.store.begin()?;
        let sale = require_sale(&*uow, sale_id)?;
        uow.commit()?;
        Ok(sale)
    }

    pub fn list(&mut self, filter: &SaleFilter) -> Result<Vec<Sale>, AppError> {
        let uow = self.store.begin()?;
        let sales = uow.search_sales(filter)?;
        uow.commit()?;
        Ok(sales)
    }
}

fn require_sale(uow: &dyn UnitOfWork, id: Uuid) -> Result<Sale, AppError> {
    uow.find_sale(id)?
        .ok_or(AppError::NotFound("venda"))
}

fn sum_received(payments: &[Payment]) -> Result<Money, DomainError> {
    payments
        .iter()
        .try_fold(Money::ZERO, |acc, payment| acc.checked_add(payment.amount()))
}