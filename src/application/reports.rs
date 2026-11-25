//! Relatórios gerenciais. Somente leitura: reutiliza os casos de uso existentes
//! e devolve tabelas prontas para exibição ou exportação.
//!
//! Relatórios de período usam a data de criação da venda ou locação, a data de
//! conclusão da entrega e a data de recebimento dos pagamentos. Relatórios de
//! situação atual ignoram o período informado.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use uuid::Uuid;

use crate::domain::delivery::DeliveryStatus;
use crate::domain::payment::{Payment, PaymentMethod, PaymentTarget};
use crate::domain::product::Product;
use crate::domain::rental::RentalStatus;
use crate::domain::repositories::{
    CustomerFilter, DeliveryFilter, ProductFilter, RentalFilter, SaleFilter,
};
use crate::domain::sale::SaleStatus;
use crate::domain::stock::MovementKind;
use crate::domain::value_objects::Money;

use super::catalog::Catalog;
use super::clock::Clock;
use super::customers::Customers;
use super::deliveries::Deliveries;
use super::error::AppError;
use super::rentals::{RentalSummary, Rentals};
use super::sales::{SaleSummary, Sales};
use super::stock::StockControl;
use super::unit_of_work::Transactor;

const PAYMENT_METHODS: [PaymentMethod; 4] = [
    PaymentMethod::Cash,
    PaymentMethod::Pix,
    PaymentMethod::CreditCard,
    PaymentMethod::DebitCard,
];

/// Relatórios disponíveis, na ordem exibida pela tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Revenue,
    SalesByPeriod,
    SalesByCustomer,
    TopProducts,
    PaymentsReceived,
    PendingBalances,
    RentalsInPeriod,
    LateRentals,
    DeliveriesDone,
    DeliveriesByDeliverer,
    ProductPositions,
    StockMovements,
}

impl ReportKind {
    pub const ALL: [ReportKind; 12] = [
        ReportKind::Revenue,
        ReportKind::SalesByPeriod,
        ReportKind::SalesByCustomer,
        ReportKind::TopProducts,
        ReportKind::PaymentsReceived,
        ReportKind::PendingBalances,
        ReportKind::RentalsInPeriod,
        ReportKind::LateRentals,
        ReportKind::DeliveriesDone,
        ReportKind::DeliveriesByDeliverer,
        ReportKind::ProductPositions,
        ReportKind::StockMovements,
    ];

    /// Índice fora da lista volta para o primeiro relatório.
    pub fn from_index(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|position| Self::ALL.get(position).copied())
            .unwrap_or(ReportKind::Revenue)
    }

    pub fn label(self) -> &'static str {
        match self {
            ReportKind::Revenue => "Faturamento e recebimentos",
            ReportKind::SalesByPeriod => "Vendas por período",
            ReportKind::SalesByCustomer => "Vendas por cliente",
            ReportKind::TopProducts => "Produtos mais vendidos",
            ReportKind::PaymentsReceived => "Pagamentos recebidos",
            ReportKind::PendingBalances => "Valores pendentes (situação atual)",
            ReportKind::RentalsInPeriod => "Locações no período",
            ReportKind::LateRentals => "Locações atrasadas (situação atual)",
            ReportKind::DeliveriesDone => "Entregas realizadas no período",
            ReportKind::DeliveriesByDeliverer => "Entregas por entregador",
            ReportKind::ProductPositions => "Posição do estoque (situação atual)",
            ReportKind::StockMovements => "Movimentações de estoque no período",
        }
    }

    /// Nome de arquivo usado na exportação.
    pub fn slug(self) -> &'static str {
        match self {
            ReportKind::Revenue => "faturamento",
            ReportKind::SalesByPeriod => "vendas-periodo",
            ReportKind::SalesByCustomer => "vendas-cliente",
            ReportKind::TopProducts => "produtos-mais-vendidos",
            ReportKind::PaymentsReceived => "pagamentos",
            ReportKind::PendingBalances => "pendencias",
            ReportKind::RentalsInPeriod => "locacoes-periodo",
            ReportKind::LateRentals => "locacoes-atrasadas",
            ReportKind::DeliveriesDone => "entregas-realizadas",
            ReportKind::DeliveriesByDeliverer => "entregas-entregador",
            ReportKind::ProductPositions => "posicao-estoque",
            ReportKind::StockMovements => "movimentacoes-estoque",
        }
    }

    pub fn uses_period(self) -> bool {
        !matches!(
            self,
            ReportKind::PendingBalances | ReportKind::LateRentals | ReportKind::ProductPositions
        )
    }
}

/// Tabela de um relatório: título, cabeçalhos, linhas e um resumo textual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub summary: String,
}

/// Totais agrupados: para clientes, `count` são vendas; para produtos, quantidade vendida.
#[derive(Debug, Default)]
struct Totals {
    count: i64,
    total: Money,
}

pub struct Reports<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Reports<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Reports<'_, S> {
    pub fn build(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        if to < from {
            return Err(AppError::Conflict(
                "a data final não pode ser anterior à inicial".to_owned(),
            ));
        }
        match kind {
            ReportKind::Revenue => self.revenue(kind, from, to),
            ReportKind::SalesByPeriod => self.sales_by_period(kind, from, to),
            ReportKind::SalesByCustomer => self.sales_by_customer(kind, from, to),
            ReportKind::TopProducts => self.top_products(kind, from, to),
            ReportKind::PaymentsReceived => self.payments_received(kind, from, to),
            ReportKind::PendingBalances => self.pending_balances(kind),
            ReportKind::RentalsInPeriod => self.rentals_in_period(kind, from, to),
            ReportKind::LateRentals => self.late_rentals(kind),
            ReportKind::DeliveriesDone => self.deliveries_done(kind, from, to),
            ReportKind::DeliveriesByDeliverer => self.deliveries_by_deliverer(kind, from, to),
            ReportKind::ProductPositions => self.product_positions(kind),
            ReportKind::StockMovements => self.stock_movements(kind, from, to),
        }
    }

    // ---------- Relatórios ----------

    fn revenue(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let (start, end) = window(from, to);
        let sales: Vec<SaleSummary> = self
            .period_sales(from, to)?
            .into_iter()
            .filter(|sale| sale.sale.status() != SaleStatus::Cancelled)
            .collect();
        let sold = money_sum(sales.iter().map(|sale| sale.total))?;
        let pending = money_sum(sales.iter().map(|sale| sale.balance))?;
        let payments = self.payments_between(start, end)?;
        let received = money_sum(payments.iter().map(|payment| payment.amount()))?;

        let mut rows = vec![
            pair("Vendas no período (sem canceladas)", sales.len().to_string()),
            pair("Total vendido", sold.to_string()),
            pair("Recebido no período", received.to_string()),
            pair("Saldo das vendas do período", pending.to_string()),
        ];
        for method in PAYMENT_METHODS {
            let total = money_sum(
                payments
                    .iter()
                    .filter(|payment| payment.method() == method)
                    .map(|payment| payment.amount()),
            )?;
            rows.push(pair(format!("Recebido em {}", method_label(method)), total.to_string()));
        }

        Ok(table(kind, &["Indicador", "Valor"], rows, period_text(from, to)))
    }

    fn sales_by_period(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let sales = self.period_sales(from, to)?;
        let names = self.customer_names()?;
        let active_total = money_sum(
            sales
                .iter()
                .filter(|sale| sale.sale.status() != SaleStatus::Cancelled)
                .map(|sale| sale.total),
        )?;

        let rows: Vec<Vec<String>> = sales
            .iter()
            .map(|sale| {
                vec![
                    sale.sale.number().to_owned(),
                    fmt_day(sale.sale.created_at()),
                    name_of(&names, sale.sale.customer_id()),
                    sale.total.to_string(),
                    sale_status_label(sale.sale.status()).to_owned(),
                ]
            })
            .collect();
        let summary = format!("{} venda(s) · total sem canceladas: {}", sales.len(), active_total);

        Ok(table(kind, &["Número", "Data", "Cliente", "Total", "Situação"], rows, summary))
    }

    fn sales_by_customer(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let names = self.customer_names()?;
        let sales = self.active_period_sales(from, to)?;

        let mut totals: HashMap<Uuid, Totals> = HashMap::new();
        for sale in &sales {
            let entry = totals.entry(sale.sale.customer_id()).or_default();
            entry.count += 1;
            entry.total = entry.total.checked_add(sale.total)?;
        }

        let mut list: Vec<(Uuid, Totals)> = totals.into_iter().collect();
                list.sort_by_key(|entry| std::cmp::Reverse(entry.1.total));

        let rows: Vec<Vec<String>> = list
            .iter()
            .map(|(id, totals)| vec![name_of(&names, *id), totals.count.to_string(), totals.total.to_string()])
            .collect();
        let summary = format!("{} cliente(s) com vendas no período", list.len());

        Ok(table(kind, &["Cliente", "Vendas", "Total"], rows, summary))
    }

    fn top_products(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let products = self.product_map()?;
        let sales = self.active_period_sales(from, to)?;

        let mut totals: HashMap<Uuid, Totals> = HashMap::new();
        for sale in &sales {
            for item in sale.sale.items() {
                let entry = totals.entry(item.product_id()).or_default();
                entry.count += item.quantity();
                entry.total = entry.total.checked_add(item.net()?)?;
            }
        }

        let mut list: Vec<(Uuid, Totals)> = totals.into_iter().collect();
        list.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(b.1.total.cmp(&a.1.total)));

        let rows: Vec<Vec<String>> = list
            .iter()
            .map(|(id, totals)| {
                vec![
                    product_label(&products, *id),
                    totals.count.to_string(),
                    totals.total.to_string(),
                ]
            })
            .collect();
        let summary = format!("{} produto(s) vendido(s) no período", list.len());

        Ok(table(kind, &["Produto", "Quantidade", "Faturamento"], rows, summary))
    }

    fn payments_received(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let (start, end) = window(from, to);
        let payments = self.payments_between(start, end)?;
        let total = money_sum(payments.iter().map(|payment| payment.amount()))?;

        let rows: Vec<Vec<String>> = payments
            .iter()
            .map(|payment| {
                vec![
                    fmt_dt(payment.paid_at()),
                    method_label(payment.method()).to_owned(),
                    origin_label(payment.target()).to_owned(),
                    payment.amount().to_string(),
                    payment.notes().to_owned(),
                ]
            })
            .collect();
        let summary = format!("{} pagamento(s) · total {}", payments.len(), total);

        Ok(table(kind, &["Data", "Forma", "Origem", "Valor", "Observação"], rows, summary))
    }

    fn pending_balances(&mut self, kind: ReportKind) -> Result<ReportTable, AppError> {
        let names = self.customer_names()?;
        let sales: Vec<SaleSummary> = self
            .sale_summaries(SaleFilter::default())?
            .into_iter()
            .filter(|sale| sale.sale.status() != SaleStatus::Cancelled && sale.balance > Money::ZERO)
            .collect();
        let rentals: Vec<RentalSummary> = self
            .rental_summaries()?
            .into_iter()
            .filter(|rental| rental.rental.status() != RentalStatus::Cancelled && rental.balance > Money::ZERO)
            .collect();

        let mut rows: Vec<Vec<String>> = Vec::new();
        for sale in &sales {
            rows.push(vec![
                "Venda".to_owned(),
                sale.sale.number().to_owned(),
                name_of(&names, sale.sale.customer_id()),
                sale.total.to_string(),
                sale.balance.to_string(),
            ]);
        }
        for rental in &rentals {
            rows.push(vec![
                "Locação".to_owned(),
                rental.rental.number().to_owned(),
                name_of(&names, rental.rental.customer_id()),
                rental.total.to_string(),
                rental.balance.to_string(),
            ]);
        }

        let balance_total = money_sum(
            sales
                .iter()
                .map(|sale| sale.balance)
                .chain(rentals.iter().map(|rental| rental.balance)),
        )?;
        let summary = format!("{} pendência(s) · saldo total {}", rows.len(), balance_total);

        Ok(table(kind, &["Origem", "Número", "Cliente", "Total", "Saldo"], rows, summary))
    }

    fn rentals_in_period(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let names = self.customer_names()?;
        let rentals = self.rental_summaries()?;
        let selected: Vec<&RentalSummary> = rentals
            .iter()
            .filter(|rental| {
                let day = rental.rental.period().start().date();
                day >= from && day <= to
            })
            .collect();
        let active_total = money_sum(
            selected
                .iter()
                .filter(|rental| rental.rental.status() != RentalStatus::Cancelled)
                .map(|rental| rental.total),
        )?;

        let rows: Vec<Vec<String>> = selected
            .iter()
            .map(|rental| {
                vec![
                    rental.rental.number().to_owned(),
                    fmt_dt(rental.rental.period().start()),
                    name_of(&names, rental.rental.customer_id()),
                    rental_status_label(rental.rental.status(), rental.late).to_owned(),
                    rental.total.to_string(),
                ]
            })
            .collect();
        let summary = format!("{} locação(ões) · total sem canceladas: {}", selected.len(), active_total);

        Ok(table(kind, &["Número", "Retirada", "Cliente", "Situação", "Total"], rows, summary))
    }

    fn late_rentals(&mut self, kind: ReportKind) -> Result<ReportTable, AppError> {
        let names = self.customer_names()?;
        let late: Vec<RentalSummary> = self
            .rental_summaries()?
            .into_iter()
            .filter(|rental| rental.late)
            .collect();

        let rows: Vec<Vec<String>> = late
            .iter()
            .map(|rental| {
                vec![
                    rental.rental.number().to_owned(),
                    fmt_dt(rental.rental.period().end()),
                    name_of(&names, rental.rental.customer_id()),
                    rental.outstanding_units.to_string(),
                    rental.balance.to_string(),
                ]
            })
            .collect();
        let summary = format!("{} locação(ões) atrasada(s)", late.len());

        Ok(table(
            kind,
            &["Número", "Devolução prevista", "Cliente", "Unidades em aberto", "Saldo"],
            rows,
            summary,
        ))
    }

    fn deliveries_done(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let (start, end) = window(from, to);
        let names = self.customer_names()?;
        let deliverers = self.deliverer_names()?;

        let delivered = Deliveries::new(&mut *self.store, self.clock).search(&DeliveryFilter {
            status: Some(DeliveryStatus::Delivered),
            ..DeliveryFilter::default()
        })?;
        let done: Vec<_> = delivered
            .into_iter()
            .filter(|delivery| {
                delivery
                    .completed_at()
                    .map(|at| at >= start && at < end)
                    .unwrap_or(false)
            })
            .collect();

        let mut rows: Vec<Vec<String>> = Vec::with_capacity(done.len());
        for delivery in &done {
            let who = self.responsible(delivery.id())?;
            rows.push(vec![
                delivery.completed_at().map(fmt_dt).unwrap_or_default(),
                name_of(&names, delivery.customer_id()),
                delivery.received_by().unwrap_or("—").to_owned(),
                deliverer_label(&deliverers, who),
                delivery.fee().to_string(),
            ]);
        }
        let fees = money_sum(done.iter().map(|delivery| delivery.fee()))?;
        let summary = format!("{} entrega(s) concluída(s) · taxas {}", done.len(), fees);

        Ok(table(
            kind,
            &["Concluída em", "Cliente", "Recebido por", "Entregador", "Taxa"],
            rows,
            summary,
        ))
    }

    fn deliveries_by_deliverer(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let (start, end) = window(from, to);
        let deliverers = self.deliverer_names()?;
        let deliveries = Deliveries::new(&mut *self.store, self.clock).search(&DeliveryFilter {
            scheduled_from: Some(start),
            scheduled_to: Some(end),
            ..DeliveryFilter::default()
        })?;

        // [entregues, não entregues, em aberto]
        let mut counts: HashMap<Option<Uuid>, [i64; 3]> = HashMap::new();
        for delivery in &deliveries {
            let who = self.responsible(delivery.id())?;
            let slot = counts.entry(who).or_insert([0; 3]);
            match delivery.status() {
                DeliveryStatus::Delivered => slot[0] += 1,
                DeliveryStatus::NotDelivered => slot[1] += 1,
                DeliveryStatus::Cancelled => {}
                _ => slot[2] += 1,
            }
        }

        let mut lines: Vec<(String, [i64; 3])> = counts
            .into_iter()
            .map(|(who, slot)| (deliverer_label(&deliverers, who), slot))
            .collect();
        lines.sort_by(|a, b| a.0.cmp(&b.0));

        let rows: Vec<Vec<String>> = lines
            .iter()
            .map(|(name, slot)| {
                vec![
                    name.clone(),
                    slot[0].to_string(),
                    slot[1].to_string(),
                    slot[2].to_string(),
                ]
            })
            .collect();
        let summary = format!("{} entrega(s) agendada(s) no período", deliveries.len());

        Ok(table(
            kind,
            &["Entregador", "Entregues", "Não entregues", "Em aberto"],
            rows,
            summary,
        ))
    }

    fn product_positions(&mut self, kind: ReportKind) -> Result<ReportTable, AppError> {
        let mut products: Vec<Product> = self.product_map()?.into_values().collect();
        products.sort_by(|a, b| a.data().sku.cmp(&b.data().sku));

        let low = products
            .iter()
            .filter(|product| product.is_active() && product.is_low_stock())
            .count();
        let rows: Vec<Vec<String>> = products
            .iter()
            .map(|product| {
                vec![
                    product.data().sku.clone(),
                    product.data().name.clone(),
                    product.stock().to_string(),
                    product.data().min_stock.to_string(),
                    product_status_label(product).to_owned(),
                ]
            })
            .collect();
        let summary = format!(
            "{} produto(s) · {} com estoque baixo ou zerado",
            products.len(),
            low
        );

        Ok(table(
            kind,
            &["SKU", "Produto", "Estoque", "Mínimo", "Situação"],
            rows,
            summary,
        ))
    }

    fn stock_movements(&mut self, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Result<ReportTable, AppError> {
        let (start, end) = window(from, to);
        let products = self.product_map()?;

        let mut entries: Vec<(NaiveDateTime, Vec<String>)> = Vec::new();
        for product in products.values() {
            let label = product_label_of(product);
            let movements = StockControl::new(&mut *self.store, self.clock).movements_for(product.id())?;
            for movement in movements {
                let at = movement.occurred_at();
                if at >= start && at < end {
                    entries.push((
                        at,
                        vec![
                            fmt_dt(at),
                            label.clone(),
                            movement_kind_label(movement.kind()).to_owned(),
                            format!("{:+}", movement.delta()),
                            movement.balance_after().to_string(),
                        ],
                    ));
                }
            }
        }
         entries.sort_by_key(|a| std::cmp::Reverse(a.0));

        let rows: Vec<Vec<String>> = entries.into_iter().map(|(_, row)| row).collect();
        let summary = format!("{} movimentação(ões) no período", rows.len());

        Ok(table(
            kind,
            &["Data", "Produto", "Tipo", "Quantidade", "Saldo após"],
            rows,
            summary,
        ))
    }

    // ---------- Consultas auxiliares ----------

    fn sale_summaries(&mut self, filter: SaleFilter) -> Result<Vec<SaleSummary>, AppError> {
        let sales = Sales::new(&mut *self.store, self.clock).list(&filter)?;
        let mut summaries = Vec::with_capacity(sales.len());
        for sale in sales {
            summaries.push(Sales::new(&mut *self.store, self.clock).summary(sale.id())?);
        }
        Ok(summaries)
    }

    fn period_sales(&mut self, from: NaiveDate, to: NaiveDate) -> Result<Vec<SaleSummary>, AppError> {
        self.sale_summaries(SaleFilter {
            from: Some(from),
            to: Some(to),
            ..SaleFilter::default()
        })
    }

    fn active_period_sales(&mut self, from: NaiveDate, to: NaiveDate) -> Result<Vec<SaleSummary>, AppError> {
        Ok(self
            .period_sales(from, to)?
            .into_iter()
            .filter(|sale| sale.sale.status() != SaleStatus::Cancelled)
            .collect())
    }

    fn rental_summaries(&mut self) -> Result<Vec<RentalSummary>, AppError> {
        let rentals = Rentals::new(&mut *self.store, self.clock).list(&RentalFilter::default())?;
        let mut summaries = Vec::with_capacity(rentals.len());
        for rental in rentals {
            summaries.push(Rentals::new(&mut *self.store, self.clock).summary(rental.id())?);
        }
        Ok(summaries)
    }

    fn payments_between(&mut self, start: NaiveDateTime, end: NaiveDateTime) -> Result<Vec<Payment>, AppError> {
        let uow = self.store.begin()?;
        let payments = uow.payments_between(start, end)?;
        uow.commit()?;
        Ok(payments)
    }

    fn customer_names(&mut self) -> Result<HashMap<Uuid, String>, AppError> {
        let customers = Customers::new(&mut *self.store, self.clock).search(&CustomerFilter {
            text: None,
            include_inactive: true,
        })?;
        Ok(customers
            .into_iter()
            .map(|customer| (customer.id(), customer.data().name.clone()))
            .collect())
    }

    fn deliverer_names(&mut self) -> Result<HashMap<Uuid, String>, AppError> {
        let deliverers = Deliveries::new(&mut *self.store, self.clock).list_deliverers(false)?;
        Ok(deliverers
            .into_iter()
            .map(|deliverer| (deliverer.id(), deliverer.data().name.clone()))
            .collect())
    }

    fn product_map(&mut self) -> Result<HashMap<Uuid, Product>, AppError> {
        let products = Catalog::new(&mut *self.store, self.clock).search_products(&ProductFilter {
            include_inactive: true,
            ..ProductFilter::default()
        })?;
        Ok(products.into_iter().map(|product| (product.id(), product)).collect())
    }

    /// Entregador vinculado à entrega: o da atribuição mais recente, ativa ou não.
    fn responsible(&mut self, delivery_id: Uuid) -> Result<Option<Uuid>, AppError> {
        let history = Deliveries::new(&mut *self.store, self.clock).history(delivery_id)?;
        Ok(history.assignments.last().map(|assignment| assignment.deliverer_id()))
    }
}

// ---------- Funções auxiliares ----------

/// Intervalo `[início do dia inicial, início do dia seguinte ao final)`.
fn window(from: NaiveDate, to: NaiveDate) -> (NaiveDateTime, NaiveDateTime) {
    (
        from.and_time(NaiveTime::MIN),
        to.and_time(NaiveTime::MIN) + Duration::days(1),
    )
}

fn money_sum(mut values: impl Iterator<Item = Money>) -> Result<Money, AppError> {
    values
        .try_fold(Money::ZERO, |acc, value| acc.checked_add(value))
        .map_err(AppError::from)
}

fn pair(label: impl Into<String>, value: impl Into<String>) -> Vec<String> {
    vec![label.into(), value.into()]
}

fn table(kind: ReportKind, headers: &[&str], rows: Vec<Vec<String>>, summary: String) -> ReportTable {
    ReportTable {
        title: kind.label().to_owned(),
        headers: headers.iter().map(|header| (*header).to_owned()).collect(),
        rows,
        summary,
    }
}

fn period_text(from: NaiveDate, to: NaiveDate) -> String {
    format!(
        "Período de {} a {}",
        from.format("%d/%m/%Y"),
        to.format("%d/%m/%Y")
    )
}

fn fmt_day(value: NaiveDateTime) -> String {
    value.format("%d/%m/%Y").to_string()
}

fn fmt_dt(value: NaiveDateTime) -> String {
    value.format("%d/%m/%Y %H:%M").to_string()
}

fn name_of(names: &HashMap<Uuid, String>, id: Uuid) -> String {
    names.get(&id).cloned().unwrap_or_else(|| "—".to_owned())
}

fn product_label(products: &HashMap<Uuid, Product>, id: Uuid) -> String {
    products
        .get(&id)
        .map(product_label_of)
        .unwrap_or_else(|| "—".to_owned())
}

fn product_label_of(product: &Product) -> String {
    format!("{} — {}", product.data().sku, product.data().name)
}

fn deliverer_label(names: &HashMap<Uuid, String>, who: Option<Uuid>) -> String {
    match who {
        Some(id) => names.get(&id).cloned().unwrap_or_else(|| "—".to_owned()),
        None => "Sem entregador".to_owned(),
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

fn origin_label(target: PaymentTarget) -> &'static str {
    match target {
        PaymentTarget::Sale(_) => "Venda",
        PaymentTarget::Rental(_) => "Locação",
    }
}

fn sale_status_label(status: SaleStatus) -> &'static str {
    match status {
        SaleStatus::Confirmed => "Confirmada",
        SaleStatus::Completed => "Concluída",
        SaleStatus::Cancelled => "Cancelada",
    }
}

fn rental_status_label(status: RentalStatus, late: bool) -> &'static str {
    match status {
        RentalStatus::Reserved => "Reservada",
        RentalStatus::Active if late => "Atrasada",
        RentalStatus::Active => "Retirada",
        RentalStatus::Closed => "Encerrada",
        RentalStatus::Cancelled => "Cancelada",
    }
}

fn product_status_label(product: &Product) -> &'static str {
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

fn movement_kind_label(kind: MovementKind) -> &'static str {
    match kind {
        MovementKind::Entry => "Entrada",
        MovementKind::Exit => "Saída",
        MovementKind::Adjustment => "Ajuste",
        MovementKind::Reversal => "Estorno",
    }
}