use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use super::audit::AuditEntry;
use super::category::Category;
use super::customer::Customer;
use super::deliverer::Deliverer;
use super::delivery::{
    Delivery, DeliveryAssignment, DeliveryHistory, DeliveryStatus, ScheduleEntry, StatusChange,
};
use super::errors::RepositoryError;
use super::order::{Order, OrderStatus};
use super::payment::Payment;
use super::product::Product;
use super::rental::{Rental, RentalStatus, ReturnEvent};
use super::rental_item::RentalItem;
use super::sale::{Sale, SaleStatus};
use super::stock::StockMovement;

// Contratos de persistência. A infraestrutura implementa; o domínio não conhece SQL.
pub trait CategoryRepository {
    fn insert_category(&mut self, category: &Category) -> Result<(), RepositoryError>;
    fn update_category(&mut self, category: &Category) -> Result<(), RepositoryError>;
    fn find_category(&self, id: Uuid) -> Result<Option<Category>, RepositoryError>;
    fn list_categories(&self, only_active: bool) -> Result<Vec<Category>, RepositoryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockAlert {
    // Saldo igual ou abaixo do mínimo.
    LowStock,
    // Saldo zerado.
    OutOfStock,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductFilter {
    /// Busca parcial por nome ou SKU, sem diferenciar acentos nem maiúsculas.
    pub text: Option<String>,
    pub category_id: Option<Uuid>,
    pub include_inactive: bool,
    pub alert: Option<StockAlert>,
}

pub trait ProductRepository {
    fn insert_product(&mut self, product: &Product) -> Result<(), RepositoryError>;
    /// Não altera o saldo: o estoque só muda por `StockRepository::record_stock_movement`.
    fn update_product(&mut self, product: &Product) -> Result<(), RepositoryError>;
    fn find_product(&self, id: Uuid) -> Result<Option<Product>, RepositoryError>;
    fn search_products(&self, filter: &ProductFilter) -> Result<Vec<Product>, RepositoryError>;
}

pub trait StockRepository {
    /// Grava a movimentação e o novo saldo. `previous_balance` é o saldo lido na mesma
    /// unidade de trabalho; a atualização só ocorre se ele ainda for o saldo persistido.
    fn record_stock_movement(
        &mut self,
        movement: &StockMovement,
        previous_balance: i64,
    ) -> Result<(), RepositoryError>;

    fn movements_for(&self, product_id: Uuid) -> Result<Vec<StockMovement>, RepositoryError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomerFilter {
    /// Busca por nome (sem acento), documento ou telefone.
    pub text: Option<String>,
    pub include_inactive: bool,
}

pub trait CustomerRepository {
    fn insert_customer(&mut self, customer: &Customer) -> Result<(), RepositoryError>;
    fn update_customer(&mut self, customer: &Customer) -> Result<(), RepositoryError>;
    fn find_customer(&self, id: Uuid) -> Result<Option<Customer>, RepositoryError>;
    fn search_customers(&self, filter: &CustomerFilter) -> Result<Vec<Customer>, RepositoryError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderFilter {
    pub customer_id: Option<Uuid>,
    pub status: Option<OrderStatus>,
}

pub trait OrderRepository {
    /// Grava somente o cabeçalho do pedido. Os itens são gravados por `replace_order_items`.
    fn insert_order(&mut self, order: &Order) -> Result<(), RepositoryError>;
    /// Atualiza status, origem, modalidade, endereço, desconto e observações.
    fn update_order(&mut self, order: &Order) -> Result<(), RepositoryError>;
    /// Substitui todos os itens. Aceito somente enquanto o pedido estiver em elaboração.
    fn replace_order_items(&mut self, order: &Order) -> Result<(), RepositoryError>;
    fn find_order(&self, id: Uuid) -> Result<Option<Order>, RepositoryError>;
    fn search_orders(&self, filter: &OrderFilter) -> Result<Vec<Order>, RepositoryError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaleFilter {
    pub customer_id: Option<Uuid>,
    pub status: Option<SaleStatus>,
    /// Número exato, por exemplo `V-2026-0001`.
    pub number: Option<String>,
    /// Período pela data de criação, inclusive.
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

pub trait SaleRepository {
    /// Grava a venda e seus itens. Os movimentos de estoque devem ser gravados antes.
    fn insert_sale(&mut self, sale: &Sale) -> Result<(), RepositoryError>;
    /// Atualiza somente situação, motivo de cancelamento e datas.
    fn update_sale(&mut self, sale: &Sale) -> Result<(), RepositoryError>;
    fn find_sale(&self, id: Uuid) -> Result<Option<Sale>, RepositoryError>;
    fn search_sales(&self, filter: &SaleFilter) -> Result<Vec<Sale>, RepositoryError>;
}

pub trait PaymentRepository {
    fn insert_payment(&mut self, payment: &Payment) -> Result<(), RepositoryError>;
    fn payments_for_sale(&self, sale_id: Uuid) -> Result<Vec<Payment>, RepositoryError>;
    fn payments_for_rental(&self, rental_id: Uuid) -> Result<Vec<Payment>, RepositoryError>;
}

pub trait SequenceRepository {
    /// Retorna o próximo valor da sequência nomeada e a avança.
    /// Deve ser chamado dentro da unidade de trabalho que consumirá o número.
    fn next_sequence_value(&mut self, name: &str) -> Result<i64, RepositoryError>;
}

pub trait RentalItemRepository {
    fn insert_rental_item(&mut self, item: &RentalItem) -> Result<(), RepositoryError>;
    fn update_rental_item(&mut self, item: &RentalItem) -> Result<(), RepositoryError>;
    fn find_rental_item(&self, id: Uuid) -> Result<Option<RentalItem>, RepositoryError>;
    fn list_rental_items(&self, only_active: bool) -> Result<Vec<RentalItem>, RepositoryError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RentalFilter {
    pub customer_id: Option<Uuid>,
    pub status: Option<RentalStatus>,
}

pub trait RentalRepository {
    /// Grava a locação e suas linhas.
    fn insert_rental(&mut self, rental: &Rental) -> Result<(), RepositoryError>;
    /// Atualiza situação, datas, motivo e contadores de devolução das linhas.
    fn update_rental(&mut self, rental: &Rental) -> Result<(), RepositoryError>;
    fn find_rental(&self, id: Uuid) -> Result<Option<Rental>, RepositoryError>;
    fn search_rentals(&self, filter: &RentalFilter) -> Result<Vec<Rental>, RepositoryError>;
    /// Locações reservadas ou retiradas. Base do cálculo de disponibilidade.
    fn open_rentals(&self) -> Result<Vec<Rental>, RepositoryError>;
    fn insert_return_events(&mut self, events: &[ReturnEvent]) -> Result<(), RepositoryError>;
}

pub trait AuditRepository {
    fn record_audit(&mut self, entry: &AuditEntry) -> Result<(), RepositoryError>;
}

pub trait DelivererRepository {
    fn insert_deliverer(&mut self, deliverer: &Deliverer) -> Result<(), RepositoryError>;
    fn update_deliverer(&mut self, deliverer: &Deliverer) -> Result<(), RepositoryError>;
    fn find_deliverer(&self, id: Uuid) -> Result<Option<Deliverer>, RepositoryError>;
    fn list_deliverers(&self, only_active: bool) -> Result<Vec<Deliverer>, RepositoryError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryFilter {
    pub status: Option<DeliveryStatus>,
    pub customer_id: Option<Uuid>,
    /// Entregas com atribuição ativa para este entregador.
    pub deliverer_id: Option<Uuid>,
    /// Intervalo de agendamento `[from, to)`.
    pub scheduled_from: Option<NaiveDateTime>,
    pub scheduled_to: Option<NaiveDateTime>,
}

pub trait DeliveryRepository {
    fn insert_delivery(&mut self, delivery: &Delivery) -> Result<(), RepositoryError>;
    /// Atualiza status, agendamento, conclusão, motivos e datas. Dados de origem não mudam.
    fn update_delivery(&mut self, delivery: &Delivery) -> Result<(), RepositoryError>;
    fn find_delivery(&self, id: Uuid) -> Result<Option<Delivery>, RepositoryError>;
    fn find_delivery_by_order(&self, order_id: Uuid) -> Result<Option<Delivery>, RepositoryError>;
    fn search_deliveries(&self, filter: &DeliveryFilter) -> Result<Vec<Delivery>, RepositoryError>;
    fn record_status_change(&mut self, change: &StatusChange) -> Result<(), RepositoryError>;
    fn record_schedule(&mut self, entry: &ScheduleEntry) -> Result<(), RepositoryError>;
    fn insert_assignment(&mut self, assignment: &DeliveryAssignment) -> Result<(), RepositoryError>;
    /// Usado para encerrar uma atribuição.
    fn update_assignment(&mut self, assignment: &DeliveryAssignment) -> Result<(), RepositoryError>;
    fn active_assignment(&self, delivery_id: Uuid) -> Result<Option<DeliveryAssignment>, RepositoryError>;
    fn delivery_history(&self, delivery_id: Uuid) -> Result<DeliveryHistory, RepositoryError>;
    /// Leitura entre agregados: situação da venda gerada a partir do pedido.
    fn sale_status_for_order(&self, order_id: Uuid) -> Result<Option<SaleStatus>, RepositoryError>;
}