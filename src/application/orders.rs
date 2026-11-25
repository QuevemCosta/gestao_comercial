use uuid::Uuid;

use crate::domain::customer::{Address, Customer};
use crate::domain::errors::DomainError;
use crate::domain::order::{Order, OrderItem, OrderOrigin, ReceiveMode};
use crate::domain::repositories::OrderFilter;
use crate::domain::value_objects::Money;

use super::clock::Clock;
use super::error::AppError;
use super::unit_of_work::{Transactor, UnitOfWork};

/// Item informado pela interface. O preço pode ser ajustado na negociação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderItemInput {
    pub product_id: Uuid,
    pub quantity: i64,
    pub unit_price: Money,
    pub discount: Money,
}

/// Casos de uso de pedidos. Nenhum deles altera estoque ou financeiro.
pub struct Orders<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Orders<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Orders<'_, S> {
    /// Cria um pedido em elaboração. Em entrega sem endereço informado,
    /// usa o endereço do cadastro do cliente como cópia.
    pub fn create(
        &mut self,
        customer_id: Uuid,
        origin: OrderOrigin,
        receive_mode: ReceiveMode,
        delivery_address: Option<Address>,
    ) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let customer = require_customer(&*uow, customer_id)?;
        if !customer.is_active() {
            return Err(DomainError::CustomerInactive.into());
        }

        let address = match receive_mode {
            ReceiveMode::Delivery => delivery_address.or_else(|| customer.data().address.clone()),
            ReceiveMode::Pickup => None,
        };

        let order = Order::create(Uuid::now_v7(), customer_id, origin, receive_mode, address, now)?;
        uow.insert_order(&order)?;
        uow.commit()?;
        Ok(order)
    }

    pub fn get(&mut self, order_id: Uuid) -> Result<Order, AppError> {
        let uow = self.store.begin()?;
        let order = require_order(&*uow, order_id)?;
        uow.commit()?;
        Ok(order)
    }

    pub fn list(&mut self, filter: &OrderFilter) -> Result<Vec<Order>, AppError> {
        let uow = self.store.begin()?;
        let orders = uow.search_orders(filter)?;
        uow.commit()?;
        Ok(orders)
    }

    /// Substitui os itens do pedido. Produtos precisam existir e estar ativos.
    pub fn set_items(&mut self, order_id: Uuid, inputs: Vec<OrderItemInput>) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = require_order(&*uow, order_id)?;

        let mut items = Vec::with_capacity(inputs.len());
        for input in inputs {
            let product = uow
                .find_product(input.product_id)?
                .ok_or(AppError::NotFound("produto"))?;
            if !product.is_active() {
                return Err(DomainError::ProductInactive.into());
            }
            items.push(OrderItem::new(
                Uuid::now_v7(),
                input.product_id,
                input.quantity,
                input.unit_price,
                input.discount,
            )?);
        }

        order.set_items(items, now)?;
        uow.replace_order_items(&order)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(order)
    }

    pub fn set_order_discount(&mut self, order_id: Uuid, discount: Money) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = require_order(&*uow, order_id)?;
        order.set_order_discount(discount, now)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(order)
    }

    /// Entrega sem endereço informado usa o endereço do cadastro do cliente.
    pub fn set_receive_mode(
        &mut self,
        order_id: Uuid,
        receive_mode: ReceiveMode,
        delivery_address: Option<Address>,
    ) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = require_order(&*uow, order_id)?;

        let address = match receive_mode {
            ReceiveMode::Delivery => match delivery_address {
                Some(address) => Some(address),
                None => require_customer(&*uow, order.customer_id())?.data().address.clone(),
            },
            ReceiveMode::Pickup => None,
        };

        order.set_receive_mode(receive_mode, address, now)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(order)
    }

    pub fn set_origin(&mut self, order_id: Uuid, origin: OrderOrigin) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = require_order(&*uow, order_id)?;
        order.set_origin(origin, now)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(order)
    }

    pub fn set_notes(&mut self, order_id: Uuid, notes: &str) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = require_order(&*uow, order_id)?;
        order.set_notes(notes, now)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(order)
    }

    pub fn cancel(&mut self, order_id: Uuid) -> Result<Order, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut order = require_order(&*uow, order_id)?;
        order.cancel(now)?;
        uow.update_order(&order)?;
        uow.commit()?;
        Ok(order)
    }
}

fn require_order(uow: &dyn UnitOfWork, id: Uuid) -> Result<Order, AppError> {
    uow.find_order(id)?
        .ok_or(AppError::NotFound("pedido"))
}

fn require_customer(uow: &dyn UnitOfWork, id: Uuid) -> Result<Customer, AppError> {
    uow.find_customer(id)?
        .ok_or(AppError::NotFound("cliente"))
}