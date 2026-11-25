//! Controlador da interface: traduz ações da tela em casos de uso e devolve dados
//! prontos para exibição. Não contém regras de negócio nem SQL.

use chrono::{Duration, NaiveDateTime};
use slint::SharedString;
use uuid::Uuid;

use crate::application::catalog::Catalog;
use crate::application::clock::{Clock, SystemClock};
use crate::application::customers::Customers;
use crate::application::deliveries::Deliveries;
use crate::application::orders::{OrderItemInput, Orders};
use crate::application::rentals::Rentals;
use crate::application::sales::Sales;
use crate::application::stock::StockControl;
use crate::domain::category::Category;
use crate::domain::customer::{Address, Customer, CustomerData, PersonType};
use crate::domain::order::{Order, OrderOrigin, OrderStatus, ReceiveMode};
use crate::domain::payment::{Payment, PaymentMethod, PaymentStatus};
use crate::domain::product::{Product, ProductData};
use crate::domain::repositories::{
    CustomerFilter, OrderFilter, ProductFilter, RentalFilter, SaleFilter, StockAlert,
};
use crate::domain::rental::RentalStatus;
use crate::domain::sale::SaleStatus;
use crate::domain::stock::{MovementKind, StockMovement};
use crate::domain::value_objects::Money;
use crate::infrastructure::database::Database;

use super::format::{parse_int, parse_money, plain_money};
use super::{
    BalanceRow, CategoryRow, CustomerRow, DashCard, ItemRow, MovementRow, OrderRow, PaymentRow,
    ProductRow, SaleRow,
};
pub mod logistics;
pub mod reports;
pub mod settings;
/// Dados digitados no formulário de produto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductForm {
    pub id: String,
    pub sku: String,
    pub name: String,
    pub description: String,
    pub purchase: String,
    pub sale: String,
    pub min_stock: String,
    pub unit: String,
    /// Índice no combo de categorias; 0 significa "sem categoria".
    pub category_index: i32,
}

/// Dados digitados no formulário de cliente.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomerForm {
    pub id: String,
    pub name: String,
    /// 0 = pessoa física, 1 = pessoa jurídica.
    pub person_index: i32,
    pub document: String,
    pub phone: String,
    pub whatsapp: String,
    pub street: String,
    pub neighborhood: String,
    pub city: String,
    pub notes: String,
}

/// Dados digitados no cabeçalho do pedido.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderForm {
    pub id: String,
    /// Índice entre os clientes ativos; -1 significa "nenhum".
    pub customer_index: i32,
    /// 0 = WhatsApp, 1 = presencial, 2 = outro.
    pub origin_index: i32,
    /// 0 = entrega, 1 = retirada.
    pub mode_index: i32,
    pub street: String,
    pub neighborhood: String,
    pub city: String,
    pub discount: String,
    pub notes: String,
}

/// Dados digitados para incluir um item no pedido.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemForm {
    /// Índice entre os produtos ativos.
    pub product_index: i32,
    pub quantity: String,
    /// Vazio significa usar o preço de venda do cadastro.
    pub unit_price: String,
    pub discount: String,
}

/// Estado completo de um pedido para a tela.
pub struct OrderView {
    pub form: OrderForm,
    pub status: String,
    pub totals: String,
    pub items: Vec<ItemRow>,
    pub editable: bool,
}

/// Estado completo de uma venda para a tela.
pub struct SaleView {
    pub title: String,
    pub totals: String,
    pub items: Vec<ItemRow>,
    pub payments: Vec<PaymentRow>,
}

/// Estado de pagamento de uma venda para a tela.
pub struct PaymentView {
    pub title: String,
    pub summary: String,
    pub history: Vec<PaymentRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockAction {
    Entry,
    Exit,
    Adjust,
}

pub struct Controller {
    db: Database,
    clock: SystemClock,
    categories: Vec<Category>,
    /// Índice 0 representa "sem categoria"; os demais seguem a ordem de `categories`.
    category_ids: Vec<Option<Uuid>>,
    /// Todos os clientes, inclusive inativos, para exibir nomes em pedidos antigos.
    customers: Vec<Customer>,
    /// Todos os produtos, inclusive inativos, para exibir nomes em itens antigos.
    products: Vec<Product>,
}

impl Controller {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            clock: SystemClock,
            categories: Vec::new(),
            category_ids: vec![None],
            customers: Vec::new(),
            products: Vec::new(),
        }
    }

    fn now(&self) -> NaiveDateTime {
        self.clock.now()
    }

    // ---------- Painel ----------

    pub fn dashboard(&mut self) -> Result<Vec<DashCard>, String> {
        let now = self.now();
        let today = now.date();

        let sales = Sales::new(&mut self.db, &self.clock)
            .list(&SaleFilter {
                from: Some(today),
                to: Some(today),
                ..SaleFilter::default()
            })
            .map_err(msg)?;

        let mut sales_count = 0usize;
        let mut sold = Money::ZERO;
        let mut received = Money::ZERO;
        for sale in sales.iter().filter(|sale| sale.status() != SaleStatus::Cancelled) {
            let summary = Sales::new(&mut self.db, &self.clock)
                .summary(sale.id())
                .map_err(msg)?;
            sales_count += 1;
            sold = sold.checked_add(summary.total).map_err(msg)?;
            received = received.checked_add(summary.received).map_err(msg)?;
        }
        let pending = sold.checked_sub(received).map_err(msg)?;

        let low_stock = StockControl::new(&mut self.db, &self.clock)
            .low_stock_products()
            .map_err(msg)?
            .len();

        let out_of_stock = Catalog::new(&mut self.db, &self.clock)
            .search_products(&ProductFilter {
                alert: Some(StockAlert::OutOfStock),
                ..ProductFilter::default()
            })
            .map_err(msg)?
            .len();

        let rentals = Rentals::new(&mut self.db, &self.clock)
            .list(&RentalFilter {
                status: Some(RentalStatus::Active),
                ..RentalFilter::default()
            })
            .map_err(msg)?;
        let active_rentals = rentals.len();
        let late_rentals = rentals.iter().filter(|rental| rental.is_late(now)).count();
        let returning_soon = rentals
            .iter()
            .filter(|rental| {
                let end = rental.period().end();
                end >= now && end <= now + Duration::hours(24)
            })
            .count();

        let panel = Deliveries::new(&mut self.db, &self.clock)
            .panel()
            .map_err(msg)?;

        Ok(vec![
            card("Vendas do dia", sales_count),
            card("Total vendido hoje", sold),
            card("Recebido hoje", received),
            card("A receber hoje", pending),
            card("Produtos com estoque baixo", low_stock),
            card("Produtos sem estoque", out_of_stock),
            card("Locações ativas", active_rentals),
            card("Locações com devolução em 24 h", returning_soon),
            card("Locações atrasadas", late_rentals),
            card("Entregas do dia", panel.today),
            card("Entregas atrasadas", panel.overdue),
            card("Entregas aguardando entregador", panel.awaiting_deliverer),
        ])
    }

    // ---------- Listas de apoio ----------

    /// Recarrega clientes e produtos usados em nomes e combos.
    pub fn load_lookups(&mut self) -> Result<(), String> {
        self.customers = Customers::new(&mut self.db, &self.clock)
            .search(&CustomerFilter {
                text: None,
                include_inactive: true,
            })
            .map_err(msg)?;
        self.products = Catalog::new(&mut self.db, &self.clock)
            .search_products(&ProductFilter {
                include_inactive: true,
                ..ProductFilter::default()
            })
            .map_err(msg)?;
        Ok(())
    }

    /// Clientes ativos, na ordem do combo de pedidos.
    pub fn customer_options(&self) -> Vec<SharedString> {
        self.active_customers()
            .map(|customer| SharedString::from(customer.data().name.as_str()))
            .collect()
    }

    /// Produtos ativos, na ordem do combo de itens.
    pub fn product_options(&self) -> Vec<SharedString> {
        self.active_products()
            .map(|product| {
                SharedString::from(format!("{} — {}", product.data().sku, product.data().name))
            })
            .collect()
    }

    fn active_customers(&self) -> impl Iterator<Item = &Customer> + '_ {
        self.customers.iter().filter(|customer| customer.is_active())
    }

    fn active_products(&self) -> impl Iterator<Item = &Product> + '_ {
        self.products.iter().filter(|product| product.is_active())
    }

    fn customer_at(&self, index: i32) -> Option<Uuid> {
        usize::try_from(index)
            .ok()
            .and_then(|position| self.active_customers().nth(position))
            .map(|customer| customer.id())
    }

    fn product_at(&self, index: i32) -> Option<&Product> {
        usize::try_from(index)
            .ok()
            .and_then(|position| self.active_products().nth(position))
    }

    fn customer_index_of(&self, id: Uuid) -> i32 {
        self.active_customers()
            .position(|customer| customer.id() == id)
            .map(|position| position as i32)
            .unwrap_or(-1)
    }

    fn customer_name(&self, id: Uuid) -> String {
        self.customers
            .iter()
            .find(|customer| customer.id() == id)
            .map(|customer| customer.data().name.clone())
            .unwrap_or_else(|| "—".to_owned())
    }

    fn product_name(&self, id: Uuid) -> String {
        self.products
            .iter()
            .find(|product| product.id() == id)
            .map(|product| product.data().name.clone())
            .unwrap_or_else(|| "—".to_owned())
    }

    // ---------- Categorias ----------

    /// Recarrega as categorias do banco e reconstrói os índices do combo.
    pub fn load_categories(&mut self) -> Result<(), String> {
        self.categories = Catalog::new(&mut self.db, &self.clock)
            .list_categories(false)
            .map_err(msg)?;
        self.category_ids = std::iter::once(None)
            .chain(self.categories.iter().map(|category| Some(category.id())))
            .collect();
        Ok(())
    }

    pub fn category_options(&self) -> Vec<SharedString> {
        std::iter::once("Sem categoria".to_owned())
            .chain(self.categories.iter().map(|category| {
                if category.is_active() {
                    category.name().to_owned()
                } else {
                    format!("{} (inativa)", category.name())
                }
            }))
            .map(SharedString::from)
            .collect()
    }

    pub fn category_rows(&self) -> Vec<CategoryRow> {
        self.categories
            .iter()
            .map(|category| {
                let status = if category.is_active() { "Ativa" } else { "Inativa" };
                CategoryRow {
                    id: category.id().to_string().into(),
                    name: category.name().into(),
                    status: status.into(),
                }
            })
            .collect()
    }

    pub fn category_name(&self, id: &str) -> Option<String> {
        let id = Uuid::parse_str(id.trim()).ok()?;
        self.categories
            .iter()
            .find(|category| category.id() == id)
            .map(|category| category.name().to_owned())
    }

    pub fn save_category(&mut self, category_id: &str, name: &str) -> Result<String, String> {
        if category_id.trim().is_empty() {
            Catalog::new(&mut self.db, &self.clock)
                .create_category(name)
                .map_err(msg)?;
            Ok("Categoria criada.".to_owned())
        } else {
            let id = parse_id(category_id)?;
            Catalog::new(&mut self.db, &self.clock)
                .rename_category(id, name)
                .map_err(msg)?;
            Ok("Categoria atualizada.".to_owned())
        }
    }

    pub fn toggle_category(&mut self, category_id: &str) -> Result<String, String> {
        let id = parse_id(category_id)?;
        self.load_categories()?;
        let currently_active = self
            .categories
            .iter()
            .find(|category| category.id() == id)
            .ok_or("categoria não encontrada")?
            .is_active();

        Catalog::new(&mut self.db, &self.clock)
            .set_category_active(id, !currently_active)
            .map_err(msg)?;

        let text = if currently_active {
            "Categoria desativada."
        } else {
            "Categoria ativada."
        };
        Ok(text.to_owned())
    }

    // ---------- Produtos ----------

    pub fn product_rows(&mut self, search: &str) -> Result<Vec<ProductRow>, String> {
        let filter = ProductFilter {
            text: non_empty(search),
            include_inactive: true,
            ..ProductFilter::default()
        };
        let products = Catalog::new(&mut self.db, &self.clock)
            .search_products(&filter)
            .map_err(msg)?;
        Ok(products.iter().map(|product| self.product_row(product)).collect())
    }

    pub fn save_product(&mut self, form: ProductForm) -> Result<String, String> {
        let category_id = self.category_at(form.category_index);
        let data = ProductData {
            sku: form.sku,
            name: form.name,
            description: form.description,
            category_id,
            purchase_price: parse_money("preço de compra", &form.purchase)?,
            sale_price: parse_money("preço de venda", &form.sale)?,
            min_stock: parse_int("estoque mínimo", &form.min_stock)?,
            unit: form.unit,
        };

        if form.id.trim().is_empty() {
            Catalog::new(&mut self.db, &self.clock)
                .create_product(data)
                .map_err(msg)?;
            Ok("Produto cadastrado.".to_owned())
        } else {
            let id = parse_id(&form.id)?;
            Catalog::new(&mut self.db, &self.clock)
                .update_product(id, data)
                .map_err(msg)?;
            Ok("Produto atualizado.".to_owned())
        }
    }

    pub fn load_product(&mut self, product_id: &str) -> Result<ProductForm, String> {
        let id = parse_id(product_id)?;
        let product = Catalog::new(&mut self.db, &self.clock)
            .get_product(id)
            .map_err(msg)?;
        let data = product.data();
        Ok(ProductForm {
            id: product.id().to_string(),
            sku: data.sku.clone(),
            name: data.name.clone(),
            description: data.description.clone(),
            purchase: plain_money(data.purchase_price),
            sale: plain_money(data.sale_price),
            min_stock: data.min_stock.to_string(),
            unit: data.unit.clone(),
            category_index: self.category_index_of_category(data.category_id),
        })
    }

    pub fn toggle_product(&mut self, product_id: &str) -> Result<String, String> {
        let id = parse_id(product_id)?;
        let mut catalog = Catalog::new(&mut self.db, &self.clock);
        let current = catalog.get_product(id).map_err(msg)?;
        let active = !current.is_active();
        catalog.set_product_active(id, active).map_err(msg)?;
        let text = if active {
            "Produto ativado."
        } else {
            "Produto desativado."
        };
        Ok(text.to_owned())
    }

    /// Lista de produtos para a tela de estoque (somente ativos).
    pub fn stock_items(&mut self) -> Result<Vec<ProductRow>, String> {
        let products = Catalog::new(&mut self.db, &self.clock)
            .search_products(&ProductFilter::default())
            .map_err(msg)?;
        Ok(products.iter().map(|product| self.product_row(product)).collect())
    }

    fn product_row(&self, product: &Product) -> ProductRow {
        let data = product.data();
        ProductRow {
            id: product.id().to_string().into(),
            sku: data.sku.as_str().into(),
            name: data.name.as_str().into(),
            category: self.category_label(data.category_id).into(),
            sale_price: data.sale_price.to_string().into(),
            stock: product.stock().to_string().into(),
            status: product_status(product).into(),
        }
    }

    fn category_label(&self, id: Option<Uuid>) -> String {
        id.and_then(|id| self.categories.iter().find(|category| category.id() == id))
            .map(|category| category.name().to_owned())
            .unwrap_or_else(|| "—".to_owned())
    }

    fn category_at(&self, index: i32) -> Option<Uuid> {
        usize::try_from(index)
            .ok()
            .and_then(|position| self.category_ids.get(position).copied().flatten())
    }

    fn category_index_of_category(&self, id: Option<Uuid>) -> i32 {
        self.category_ids
            .iter()
            .position(|candidate| *candidate == id)
            .map(|position| position as i32)
            .unwrap_or(0)
    }

    // ---------- Estoque ----------

    /// Seleciona um produto e devolve o rótulo do cabeçalho e o histórico de movimentações.
    pub fn select_stock_item(
        &mut self,
        product_id: &str,
    ) -> Result<(String, Vec<MovementRow>), String> {
        let id = parse_id(product_id)?;
        let product = Catalog::new(&mut self.db, &self.clock)
            .get_product(id)
            .map_err(msg)?;
        let label = format!("{} — {}", product.data().sku, product.data().name);
        let rows = self.stock_movements(product_id)?;
        Ok((label, rows))
    }

    pub fn stock_movements(&mut self, product_id: &str) -> Result<Vec<MovementRow>, String> {
        let id = parse_id(product_id)?;
        let movements = StockControl::new(&mut self.db, &self.clock)
            .movements_for(id)
            .map_err(msg)?;
        // Mais recentes primeiro.
        Ok(movements.iter().rev().map(movement_row).collect())
    }

    pub fn stock_operation(
        &mut self,
        product_id: &str,
        action: StockAction,
        amount: &str,
        reason: &str,
    ) -> Result<String, String> {
        let id = parse_id(product_id)?;
        let quantity = parse_int("quantidade", amount)?;
        let mut stock = StockControl::new(&mut self.db, &self.clock);
        match action {
            StockAction::Entry => {
                stock.register_entry(id, quantity, reason).map_err(msg)?;
                Ok("Entrada registrada.".to_owned())
            }
            StockAction::Exit => {
                stock.register_exit(id, quantity, reason).map_err(msg)?;
                Ok("Saída registrada.".to_owned())
            }
            StockAction::Adjust => {
                stock.adjust_stock(id, quantity, reason).map_err(msg)?;
                Ok(format!("Estoque ajustado para {quantity} unidade(s)."))
            }
        }
    }

    pub fn stock_alert_text(&mut self) -> Result<String, String> {
        let low = StockControl::new(&mut self.db, &self.clock)
            .low_stock_products()
            .map_err(msg)?;
        Ok(if low.is_empty() {
            "Nenhum produto com estoque baixo.".to_owned()
        } else {
            format!("{} produto(s) com estoque baixo ou zerado.", low.len())
        })
    }

    // ---------- Clientes ----------

    pub fn customer_rows(&mut self, search: &str) -> Result<Vec<CustomerRow>, String> {
        let filter = CustomerFilter {
            text: non_empty(search),
            include_inactive: true,
        };
        let customers = Customers::new(&mut self.db, &self.clock)
            .search(&filter)
            .map_err(msg)?;
        Ok(customers.iter().map(customer_row).collect())
    }

    pub fn save_customer(&mut self, form: CustomerForm) -> Result<String, String> {
        let address = address_from(&form.street, &form.neighborhood, &form.city)?;
        let data = CustomerData {
            person_type: if form.person_index == 1 {
                PersonType::Company
            } else {
                PersonType::Individual
            },
            name: form.name,
            document: non_empty(&form.document),
            phone: non_empty(&form.phone),
            whatsapp: non_empty(&form.whatsapp),
            address,
            notes: form.notes,
        };

        if form.id.trim().is_empty() {
            Customers::new(&mut self.db, &self.clock)
                .register(data)
                .map_err(msg)?;
            Ok("Cliente cadastrado.".to_owned())
        } else {
            let id = parse_id(&form.id)?;
            Customers::new(&mut self.db, &self.clock)
                .update(id, data)
                .map_err(msg)?;
            Ok("Cliente atualizado.".to_owned())
        }
    }

    pub fn load_customer(&mut self, customer_id: &str) -> Result<CustomerForm, String> {
        let id = parse_id(customer_id)?;
        let customer = Customers::new(&mut self.db, &self.clock)
            .get(id)
            .map_err(msg)?;
        let data = customer.data();
        let (street, neighborhood, city) = match &data.address {
            Some(address) => (
                address.street().to_owned(),
                address.neighborhood().to_owned(),
                address.city().to_owned(),
            ),
            None => Default::default(),
        };
        Ok(CustomerForm {
            id: customer.id().to_string(),
            name: data.name.clone(),
            person_index: match data.person_type {
                PersonType::Individual => 0,
                PersonType::Company => 1,
            },
            document: data.document.clone().unwrap_or_default(),
            phone: data.phone.clone().unwrap_or_default(),
            whatsapp: data.whatsapp.clone().unwrap_or_default(),
            street,
            neighborhood,
            city,
            notes: data.notes.clone(),
        })
    }

    pub fn toggle_customer(&mut self, customer_id: &str) -> Result<String, String> {
        let id = parse_id(customer_id)?;
        let mut customers = Customers::new(&mut self.db, &self.clock);
        let current = customers.get(id).map_err(msg)?;
        let active = !current.is_active();
        customers.set_active(id, active).map_err(msg)?;
        let text = if active {
            "Cliente ativado."
        } else {
            "Cliente desativado."
        };
        Ok(text.to_owned())
    }

    // ---------- Pedidos ----------

    pub fn order_rows(&mut self) -> Result<Vec<OrderRow>, String> {
        self.load_lookups()?;
        let orders = Orders::new(&mut self.db, &self.clock)
            .list(&OrderFilter::default())
            .map_err(msg)?;
        // Mais recentes primeiro.
        orders.iter().rev().map(|order| self.order_row(order)).collect()
    }

    fn order_row(&self, order: &Order) -> Result<OrderRow, String> {
        Ok(OrderRow {
            id: order.id().to_string().into(),
            created: order.created_at().format("%d/%m/%Y %H:%M").to_string().into(),
            customer: self.customer_name(order.customer_id()).into(),
            status: order_status_label(order.status()).into(),
            total: order.total().map_err(msg)?.to_string().into(),
        })
    }

    pub fn load_order(&mut self, order_id: &str) -> Result<OrderView, String> {
        self.load_lookups()?;
        let id = parse_id(order_id)?;
        let order = Orders::new(&mut self.db, &self.clock)
            .get(id)
            .map_err(msg)?;
        self.order_view(&order)
    }

    fn order_view(&self, order: &Order) -> Result<OrderView, String> {
        let (street, neighborhood, city) = match order.delivery_address() {
            Some(address) => (
                address.street().to_owned(),
                address.neighborhood().to_owned(),
                address.city().to_owned(),
            ),
            None => Default::default(),
        };

        let form = OrderForm {
            id: order.id().to_string(),
            customer_index: self.customer_index_of(order.customer_id()),
            origin_index: origin_index(order.origin()),
            mode_index: mode_index(order.receive_mode()),
            street,
            neighborhood,
            city,
            discount: plain_money(order.order_discount()),
            notes: order.notes().to_owned(),
        };

        let items = order
            .items()
            .iter()
            .enumerate()
            .map(|(index, item)| -> Result<ItemRow, String> {
                Ok(ItemRow {
                    index: index as i32,
                    product: self.product_name(item.product_id()).into(),
                    quantity: item.quantity().to_string().into(),
                    unit_price: item.unit_price().to_string().into(),
                    discount: item.discount().to_string().into(),
                    total: item.net().map_err(msg)?.to_string().into(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let totals = format!(
            "Subtotal: {} | Desconto em itens: {} | Desconto do pedido: {} | Total: {}",
            order.subtotal().map_err(msg)?,
            order.items_discount().map_err(msg)?,
            order.order_discount(),
            order.total().map_err(msg)?,
        );

        Ok(OrderView {
            form,
            status: order_status_label(order.status()).to_owned(),
            totals,
            items,
            editable: order.status() == OrderStatus::Draft,
        })
    }

    /// Cria o pedido (se novo) ou atualiza o cabeçalho (se existente). Devolve o identificador.
    pub fn save_order(&mut self, form: OrderForm) -> Result<String, String> {
        self.load_lookups()?;
        let address = address_from(&form.street, &form.neighborhood, &form.city)?;
        let discount = match form.discount.trim() {
            "" => Money::ZERO,
            _ => parse_money("desconto do pedido", &form.discount)?,
        };
        let origin = origin_from_index(form.origin_index);
        let mode = mode_from_index(form.mode_index);

        if form.id.trim().is_empty() {
            let customer_id = self
                .customer_at(form.customer_index)
                .ok_or_else(|| "selecione o cliente".to_owned())?;
            if discount > Money::ZERO {
                return Err("informe o desconto depois de incluir os itens do pedido".to_owned());
            }
            let mut orders = Orders::new(&mut self.db, &self.clock);
            let order = orders
                .create(customer_id, origin, mode, address)
                .map_err(msg)?;
            if !form.notes.trim().is_empty() {
                orders.set_notes(order.id(), &form.notes).map_err(msg)?;
            }
            Ok(order.id().to_string())
        } else {
            let id = parse_id(&form.id)?;
            let mut orders = Orders::new(&mut self.db, &self.clock);
            orders.set_origin(id, origin).map_err(msg)?;
            orders.set_receive_mode(id, mode, address).map_err(msg)?;
            orders.set_order_discount(id, discount).map_err(msg)?;
            orders.set_notes(id, &form.notes).map_err(msg)?;
            Ok(id.to_string())
        }
    }

    pub fn add_order_item(&mut self, order_id: &str, form: ItemForm) -> Result<(), String> {
        self.load_lookups()?;
        let id = parse_id(order_id)?;

        let (product_id, default_price) = {
            let product = self
                .product_at(form.product_index)
                .ok_or_else(|| "selecione o produto".to_owned())?;
            (product.id(), product.data().sale_price)
        };

        let quantity = parse_int("quantidade", &form.quantity)?;
        let unit_price = match form.unit_price.trim() {
            "" => default_price,
            _ => parse_money("preço unitário", &form.unit_price)?,
        };
        let discount = match form.discount.trim() {
            "" => Money::ZERO,
            _ => parse_money("desconto do item", &form.discount)?,
        };

        let mut orders = Orders::new(&mut self.db, &self.clock);
        let current = orders.get(id).map_err(msg)?;
        let mut inputs = inputs_of(&current);
        inputs.push(OrderItemInput {
            product_id,
            quantity,
            unit_price,
            discount,
        });
        orders.set_items(id, inputs).map_err(msg)?;
        Ok(())
    }

    pub fn remove_order_item(&mut self, order_id: &str, position: i32) -> Result<(), String> {
        let id = parse_id(order_id)?;
        let index = usize::try_from(position).map_err(|_| "item inválido".to_owned())?;

        let mut orders = Orders::new(&mut self.db, &self.clock);
        let current = orders.get(id).map_err(msg)?;
        let mut inputs = inputs_of(&current);
        if index < inputs.len() {
            inputs.remove(index);
        }
        orders.set_items(id, inputs).map_err(msg)?;
        Ok(())
    }

    /// Confirma o pedido: gera a venda e baixa o estoque.
    pub fn confirm_order(&mut self, order_id: &str) -> Result<String, String> {
        let id = parse_id(order_id)?;
        let sale = Sales::new(&mut self.db, &self.clock)
            .confirm_order(id)
            .map_err(msg)?;
        Ok(format!("Venda {} confirmada e estoque baixado.", sale.number()))
    }

    pub fn cancel_order(&mut self, order_id: &str) -> Result<String, String> {
        let id = parse_id(order_id)?;
        Orders::new(&mut self.db, &self.clock)
            .cancel(id)
            .map_err(msg)?;
        Ok("Pedido cancelado.".to_owned())
    }

    // ---------- Vendas ----------

    /// Lista vendas. Com `number` preenchido, filtra pelo número exato.
    pub fn sale_rows(&mut self, number: &str) -> Result<Vec<SaleRow>, String> {
        self.load_lookups()?;
        let filter = SaleFilter {
            number: non_empty(number),
            ..SaleFilter::default()
        };
        let sales = Sales::new(&mut self.db, &self.clock)
            .list(&filter)
            .map_err(msg)?;

        let mut rows = Vec::with_capacity(sales.len());
        for sale in sales.iter().rev() {
            let summary = Sales::new(&mut self.db, &self.clock)
                .summary(sale.id())
                .map_err(msg)?;
            rows.push(SaleRow {
                id: sale.id().to_string().into(),
                number: sale.number().into(),
                customer: self.customer_name(sale.customer_id()).into(),
                status: sale_status_label(sale.status()).into(),
                total: summary.total.to_string().into(),
                payment: payment_label(summary.payment_status).into(),
            });
        }
        Ok(rows)
    }

    pub fn load_sale(&mut self, sale_id: &str) -> Result<SaleView, String> {
        self.load_lookups()?;
        let id = parse_id(sale_id)?;
        let summary = Sales::new(&mut self.db, &self.clock)
            .summary(id)
            .map_err(msg)?;
        let sale = &summary.sale;

        let items = sale
            .items()
            .iter()
            .enumerate()
            .map(|(index, item)| -> Result<ItemRow, String> {
                Ok(ItemRow {
                    index: index as i32,
                    product: self.product_name(item.product_id()).into(),
                    quantity: item.quantity().to_string().into(),
                    unit_price: item.unit_price().to_string().into(),
                    discount: item.discount().to_string().into(),
                    total: item.net().map_err(msg)?.to_string().into(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let totals = format!(
            "Subtotal: {} | Desconto em itens: {} | Desconto da venda: {} | Total: {} | Recebido: {} | Saldo: {}",
            sale.subtotal().map_err(msg)?,
            sale.items_discount().map_err(msg)?,
            sale.order_discount(),
            summary.total,
            summary.received,
            summary.balance,
        );

        Ok(SaleView {
            title: format!(
                "Venda {} — {}",
                sale.number(),
                self.customer_name(sale.customer_id())
            ),
            totals,
            items,
            payments: summary.payments.iter().map(payment_row).collect(),
        })
    }

    pub fn complete_sale(&mut self, sale_id: &str) -> Result<String, String> {
        let id = parse_id(sale_id)?;
        Sales::new(&mut self.db, &self.clock)
            .complete(id)
            .map_err(msg)?;
        Ok("Venda marcada como concluída.".to_owned())
    }

    /// Cancela a venda e estorna o estoque. Se houver valor recebido, a mensagem alerta para tratamento manual.
    pub fn cancel_sale(&mut self, sale_id: &str, reason: &str) -> Result<String, String> {
        let id = parse_id(sale_id)?;
        let cancellation = Sales::new(&mut self.db, &self.clock)
            .cancel(id, reason)
            .map_err(msg)?;
        if cancellation.received > Money::ZERO {
            Ok(format!(
                "Venda cancelada e estoque estornado. Atenção: {} já recebido precisa de tratamento manual.",
                cancellation.received
            ))
        } else {
            Ok("Venda cancelada e estoque estornado.".to_owned())
        }
    }

    // ---------- Pagamentos ----------

    /// Vendas não canceladas com saldo pendente.
    pub fn balance_rows(&mut self) -> Result<Vec<BalanceRow>, String> {
        self.load_lookups()?;
        let sales = Sales::new(&mut self.db, &self.clock)
            .list(&SaleFilter::default())
            .map_err(msg)?;

        let mut rows = Vec::new();
        for sale in sales.iter().filter(|sale| sale.status() != SaleStatus::Cancelled) {
            let summary = Sales::new(&mut self.db, &self.clock)
                .summary(sale.id())
                .map_err(msg)?;
            if summary.balance > Money::ZERO {
                rows.push(BalanceRow {
                    id: sale.id().to_string().into(),
                    number: sale.number().into(),
                    customer: self.customer_name(sale.customer_id()).into(),
                    total: summary.total.to_string().into(),
                    balance: summary.balance.to_string().into(),
                });
            }
        }
        Ok(rows)
    }

    pub fn load_payment_view(&mut self, sale_id: &str) -> Result<PaymentView, String> {
        self.load_lookups()?;
        let id = parse_id(sale_id)?;
        let summary = Sales::new(&mut self.db, &self.clock)
            .summary(id)
            .map_err(msg)?;

        Ok(PaymentView {
            title: format!(
                "Venda {} — {}",
                summary.sale.number(),
                self.customer_name(summary.sale.customer_id())
            ),
            summary: format!(
                "Total: {} | Recebido: {} | Saldo: {} | Situação: {}",
                summary.total,
                summary.received,
                summary.balance,
                payment_label(summary.payment_status)
            ),
            history: summary.payments.iter().map(payment_row).collect(),
        })
    }

    pub fn register_payment(
        &mut self,
        sale_id: &str,
        method_index: i32,
        amount: &str,
        notes: &str,
    ) -> Result<String, String> {
        let id = parse_id(sale_id)?;
        let value = parse_money("valor do pagamento", amount)?;
        let method = payment_method_from_index(method_index);
        Sales::new(&mut self.db, &self.clock)
            .register_payment(id, method, value, notes)
            .map_err(msg)?;
        Ok("Pagamento registrado.".to_owned())
    }
}

// ---------- Funções auxiliares ----------

fn inputs_of(order: &Order) -> Vec<OrderItemInput> {
    order
        .items()
        .iter()
        .map(|item| OrderItemInput {
            product_id: item.product_id(),
            quantity: item.quantity(),
            unit_price: item.unit_price(),
            discount: item.discount(),
        })
        .collect()
}

fn origin_index(origin: OrderOrigin) -> i32 {
    match origin {
        OrderOrigin::Whatsapp => 0,
        OrderOrigin::InPerson => 1,
        OrderOrigin::Other => 2,
    }
}

fn origin_from_index(index: i32) -> OrderOrigin {
    match index {
        1 => OrderOrigin::InPerson,
        2 => OrderOrigin::Other,
        _ => OrderOrigin::Whatsapp,
    }
}

fn mode_index(mode: ReceiveMode) -> i32 {
    match mode {
        ReceiveMode::Delivery => 0,
        ReceiveMode::Pickup => 1,
    }
}

fn mode_from_index(index: i32) -> ReceiveMode {
    match index {
        1 => ReceiveMode::Pickup,
        _ => ReceiveMode::Delivery,
    }
}

fn order_status_label(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::Draft => "Em elaboração",
        OrderStatus::Confirmed => "Confirmado",
        OrderStatus::Converted => "Convertido em venda",
        OrderStatus::Cancelled => "Cancelado",
    }
}

fn sale_status_label(status: SaleStatus) -> &'static str {
    match status {
        SaleStatus::Confirmed => "Confirmada",
        SaleStatus::Completed => "Concluída",
        SaleStatus::Cancelled => "Cancelada",
    }
}

fn payment_label(status: PaymentStatus) -> &'static str {
    match status {
        PaymentStatus::Pending => "Pendente",
        PaymentStatus::Partial => "Parcial",
        PaymentStatus::Paid => "Pago",
    }
}

fn method_label(method: PaymentMethod) -> &'static str {
    match method {
        PaymentMethod::Cash => "Dinheiro",
        PaymentMethod::Pix => "PIX",
        PaymentMethod::CreditCard => "Cartão de crédito",
        PaymentMethod::DebitCard => "Cartão de débito",
    }
}

fn payment_method_from_index(index: i32) -> PaymentMethod {
    match index {
        1 => PaymentMethod::Pix,
        2 => PaymentMethod::CreditCard,
        3 => PaymentMethod::DebitCard,
        _ => PaymentMethod::Cash,
    }
}

fn payment_row(payment: &Payment) -> PaymentRow {
    PaymentRow {
        occurred: payment.paid_at().format("%d/%m/%Y %H:%M").to_string().into(),
        method: method_label(payment.method()).into(),
        amount: payment.amount().to_string().into(),
        notes: payment.notes().into(),
    }
}

fn product_status(product: &Product) -> &'static str {
    if !product.is_active() {
        "Inativo"
    } else if product.is_out_of_stock() {
        "Sem estoque"
    } else if product.is_low_stock() {
        "Estoque baixo"
    } else {
        "Ativo"
    }
}

fn movement_row(movement: &StockMovement) -> MovementRow {
    MovementRow {
        occurred: movement
            .occurred_at()
            .format("%d/%m/%Y %H:%M")
            .to_string()
            .into(),
        kind: movement_label(movement.kind()).into(),
        delta: format!("{:+}", movement.delta()).into(),
        balance: movement.balance_after().to_string().into(),
        reason: movement.reason().into(),
    }
}

fn movement_label(kind: MovementKind) -> &'static str {
    match kind {
        MovementKind::Entry => "Entrada",
        MovementKind::Exit => "Saída",
        MovementKind::Adjustment => "Ajuste",
        MovementKind::Reversal => "Estorno",
    }
}

fn customer_row(customer: &Customer) -> CustomerRow {
    let data = customer.data();
    let status = if customer.is_active() { "Ativo" } else { "Inativo" };
    CustomerRow {
        id: customer.id().to_string().into(),
        name: data.name.as_str().into(),
        document: data.document.clone().unwrap_or_default().into(),
        phone: data
            .phone
            .clone()
            .or_else(|| data.whatsapp.clone())
            .unwrap_or_default()
            .into(),
        status: status.into(),
    }
}

/// Endereço é tudo ou nada: ou os três campos estão preenchidos, ou nenhum.
fn address_from(street: &str, neighborhood: &str, city: &str) -> Result<Option<Address>, String> {
    let empty = [street, neighborhood, city].map(|part| part.trim().is_empty());
    match empty {
        [true, true, true] => Ok(None),
        [false, false, false] => Address::new(street, neighborhood, city)
            .map(Some)
            .map_err(msg),
        _ => Err("Para salvar o endereço, informe logradouro, bairro e cidade.".to_owned()),
    }
}

fn card(label: &str, value: impl std::fmt::Display) -> DashCard {
    DashCard {
        label: label.into(),
        value: value.to_string().into(),
    }
}

fn parse_id(raw: &str) -> Result<Uuid, String> {
    Uuid::parse_str(raw.trim()).map_err(|_| "identificador inválido".to_owned())
}

fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn msg<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}