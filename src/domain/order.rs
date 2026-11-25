use std::collections::HashSet;

use chrono::NaiveDateTime;
use uuid::Uuid;

use super::customer::Address;
use super::errors::DomainError;
use super::value_objects::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    Draft,
    Confirmed,
    Converted,
    Cancelled,
}

impl OrderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderStatus::Draft => "draft",
            OrderStatus::Confirmed => "confirmed",
            OrderStatus::Converted => "converted",
            OrderStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "draft" => Some(OrderStatus::Draft),
            "confirmed" => Some(OrderStatus::Confirmed),
            "converted" => Some(OrderStatus::Converted),
            "cancelled" => Some(OrderStatus::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderOrigin {
    Whatsapp,
    InPerson,
    Other,
}

impl OrderOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderOrigin::Whatsapp => "whatsapp",
            OrderOrigin::InPerson => "in_person",
            OrderOrigin::Other => "other",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "whatsapp" => Some(OrderOrigin::Whatsapp),
            "in_person" => Some(OrderOrigin::InPerson),
            "other" => Some(OrderOrigin::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveMode {
    Delivery,
    Pickup,
}

impl ReceiveMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReceiveMode::Delivery => "delivery",
            ReceiveMode::Pickup => "pickup",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "delivery" => Some(ReceiveMode::Delivery),
            "pickup" => Some(ReceiveMode::Pickup),
            _ => None,
        }
    }
}

/// Item do pedido. Preço e desconto são valores do momento da negociação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderItem {
    id: Uuid,
    product_id: Uuid,
    quantity: i64,
    unit_price: Money,
    discount: Money,
}

impl OrderItem {
    pub fn new(
        id: Uuid,
        product_id: Uuid,
        quantity: i64,
        unit_price: Money,
        discount: Money,
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
        let item = Self { id, product_id, quantity, unit_price, discount };
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
    ) -> Self {
        Self { id, product_id, quantity, unit_price, discount }
    }

    /// Preço unitário multiplicado pela quantidade, antes do desconto.
    pub fn gross(&self) -> Result<Money, DomainError> {
        let quantity = u32::try_from(self.quantity).map_err(|_| DomainError::Overflow)?;
        self.unit_price.checked_mul_qty(quantity)
    }

    /// Valor do item após o desconto.
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
}

/// Pedido comercial. Não movimenta estoque nem gera financeiro: isso ocorre na confirmação (Etapa 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    id: Uuid,
    customer_id: Uuid,
    status: OrderStatus,
    origin: OrderOrigin,
    receive_mode: ReceiveMode,
    delivery_address: Option<Address>,
    order_discount: Money,
    notes: String,
    items: Vec<OrderItem>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl Order {
    pub fn create(
        id: Uuid,
        customer_id: Uuid,
        origin: OrderOrigin,
        receive_mode: ReceiveMode,
        delivery_address: Option<Address>,
        now: NaiveDateTime,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            customer_id,
            status: OrderStatus::Draft,
            origin,
            receive_mode,
            delivery_address: resolve_address(receive_mode, delivery_address)?,
            order_discount: Money::ZERO,
            notes: String::new(),
            items: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: Uuid,
        customer_id: Uuid,
        status: OrderStatus,
        origin: OrderOrigin,
        receive_mode: ReceiveMode,
        delivery_address: Option<Address>,
        order_discount: Money,
        notes: String,
        items: Vec<OrderItem>,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self {
            id,
            customer_id,
            status,
            origin,
            receive_mode,
            delivery_address,
            order_discount,
            notes,
            items,
            created_at,
            updated_at,
        }
    }

    /// Substitui todos os itens. Os descontos do pedido são revalidados contra os novos valores.
    pub fn set_items(&mut self, items: Vec<OrderItem>, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_draft()?;
        ensure_unique_products(&items)?;
        if self.order_discount > items_net_total(&items)? {
            return Err(DomainError::DiscountExceedsAmount);
        }
        self.items = items;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_order_discount(&mut self, discount: Money, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_draft()?;
        if discount.is_negative() {
            return Err(DomainError::NegativeValue("desconto do pedido"));
        }
        if discount > items_net_total(&self.items)? {
            return Err(DomainError::DiscountExceedsAmount);
        }
        self.order_discount = discount;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_receive_mode(
        &mut self,
        receive_mode: ReceiveMode,
        delivery_address: Option<Address>,
        now: NaiveDateTime,
    ) -> Result<(), DomainError> {
        self.ensure_draft()?;
        self.delivery_address = resolve_address(receive_mode, delivery_address)?;
        self.receive_mode = receive_mode;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_origin(&mut self, origin: OrderOrigin, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_draft()?;
        self.origin = origin;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_notes(&mut self, notes: &str, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_draft()?;
        self.notes = notes.trim().to_owned();
        self.updated_at = now;
        Ok(())
    }

    pub fn cancel(&mut self, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_draft()?;
        self.status = OrderStatus::Cancelled;
        self.updated_at = now;
        Ok(())
    }

        /// Confirma o pedido: passa a ser imutável e dá origem à venda.
    pub fn confirm(&mut self, now: NaiveDateTime) -> Result<(), DomainError> {
        self.ensure_draft()?;
        if self.items.is_empty() {
            return Err(DomainError::EmptySale);
        }
        self.status = OrderStatus::Confirmed;
        self.updated_at = now;
        Ok(())
    }

    /// Soma dos preços brutos dos itens.
    pub fn subtotal(&self) -> Result<Money, DomainError> {
        self.items
            .iter()
            .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.gross()?))
    }

    /// Soma dos descontos dos itens.
    pub fn items_discount(&self) -> Result<Money, DomainError> {
        self.items
            .iter()
            .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.discount()))
    }

    /// Total final: itens após descontos, menos o desconto do pedido.
    pub fn total(&self) -> Result<Money, DomainError> {
        items_net_total(&self.items)?.checked_sub(self.order_discount)
    }

    fn ensure_draft(&self) -> Result<(), DomainError> {
        if self.status == OrderStatus::Draft {
            Ok(())
        } else {
            Err(DomainError::OrderNotDraft)
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn customer_id(&self) -> Uuid {
        self.customer_id
    }

    pub fn status(&self) -> OrderStatus {
        self.status
    }

    pub fn origin(&self) -> OrderOrigin {
        self.origin
    }

    pub fn receive_mode(&self) -> ReceiveMode {
        self.receive_mode
    }

    pub fn delivery_address(&self) -> Option<&Address> {
        self.delivery_address.as_ref()
    }

    pub fn order_discount(&self) -> Money {
        self.order_discount
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }

    pub fn items(&self) -> &[OrderItem] {
        &self.items
    }

    pub fn created_at(&self) -> NaiveDateTime {
        self.created_at
    }

    pub fn updated_at(&self) -> NaiveDateTime {
        self.updated_at
    }
}

/// Entrega exige endereço. Retirada não mantém endereço.
fn resolve_address(
    receive_mode: ReceiveMode,
    delivery_address: Option<Address>,
) -> Result<Option<Address>, DomainError> {
    match receive_mode {
        ReceiveMode::Delivery => delivery_address
            .ok_or(DomainError::DeliveryAddressRequired)
            .map(Some),
        ReceiveMode::Pickup => Ok(None),
    }
}

fn items_net_total(items: &[OrderItem]) -> Result<Money, DomainError> {
    items
        .iter()
        .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.net()?))
}

fn ensure_unique_products(items: &[OrderItem]) -> Result<(), DomainError> {
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        if !seen.insert(item.product_id()) {
            return Err(DomainError::DuplicateProduct);
        }
    }
    Ok(())
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

    fn address() -> Address {
        Address::new("Rua A, 1", "Centro", "Fortaleza").expect("endereço válido")
    }

    fn draft(mode: ReceiveMode, address: Option<Address>) -> Result<Order, DomainError> {
        Order::create(Uuid::now_v7(), Uuid::now_v7(), OrderOrigin::Whatsapp, mode, address, now())
    }

    fn item(quantity: i64, unit: i64, discount: i64) -> Result<OrderItem, DomainError> {
        OrderItem::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            quantity,
            Money::from_cents(unit),
            Money::from_cents(discount),
        )
    }

    #[test]
    fn totals_apply_item_and_order_discounts() -> Result<(), DomainError> {
        let mut order = draft(ReceiveMode::Pickup, None)?;
        order.set_items(vec![item(3, 1000, 500)?, item(2, 2500, 0)?], now())?;
        order.set_order_discount(Money::from_cents(1000), now())?;

        assert_eq!(order.subtotal()?, Money::from_cents(8000));
        assert_eq!(order.items_discount()?, Money::from_cents(500));
        assert_eq!(order.total()?, Money::from_cents(6500));
        Ok(())
    }

    #[test]
    fn item_discount_cannot_exceed_gross() {
        assert_eq!(item(1, 1000, 1500).unwrap_err(), DomainError::DiscountExceedsAmount);
    }

    #[test]
    fn order_discount_cannot_exceed_items() -> Result<(), DomainError> {
        let mut order = draft(ReceiveMode::Pickup, None)?;
        order.set_items(vec![item(1, 1000, 0)?], now())?;
        assert_eq!(
            order.set_order_discount(Money::from_cents(1001), now()),
            Err(DomainError::DiscountExceedsAmount)
        );
        Ok(())
    }

    #[test]
    fn shrinking_items_revalidates_order_discount() -> Result<(), DomainError> {
        let mut order = draft(ReceiveMode::Pickup, None)?;
        order.set_items(vec![item(1, 5000, 0)?], now())?;
        order.set_order_discount(Money::from_cents(3000), now())?;
        assert_eq!(
            order.set_items(vec![item(1, 1000, 0)?], now()),
            Err(DomainError::DiscountExceedsAmount)
        );
        Ok(())
    }

    #[test]
    fn delivery_requires_address_and_pickup_drops_it() -> Result<(), DomainError> {
        assert_eq!(
            draft(ReceiveMode::Delivery, None),
            Err(DomainError::DeliveryAddressRequired)
        );
        let pickup = draft(ReceiveMode::Pickup, Some(address()))?;
        assert!(pickup.delivery_address().is_none());
        Ok(())
    }

    #[test]
    fn duplicate_products_are_rejected() -> Result<(), DomainError> {
        let product = Uuid::now_v7();
        let first = OrderItem::new(Uuid::now_v7(), product, 1, Money::from_cents(100), Money::ZERO)?;
        let second = OrderItem::new(Uuid::now_v7(), product, 2, Money::from_cents(100), Money::ZERO)?;
        let mut order = draft(ReceiveMode::Pickup, None)?;
        assert_eq!(order.set_items(vec![first, second], now()), Err(DomainError::DuplicateProduct));
        Ok(())
    }

    #[test]
    fn cancelled_order_is_not_editable() -> Result<(), DomainError> {
        let mut order = draft(ReceiveMode::Pickup, None)?;
        order.cancel(now())?;
        assert_eq!(order.set_items(Vec::new(), now()), Err(DomainError::OrderNotDraft));
        assert_eq!(order.status(), OrderStatus::Cancelled);
        Ok(())
    }
}