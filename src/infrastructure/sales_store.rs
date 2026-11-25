use chrono::NaiveDateTime;
use rusqlite::types::Type;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::errors::RepositoryError;
use crate::domain::payment::{Payment, PaymentMethod, PaymentTarget};
use crate::domain::repositories::{PaymentRepository, SaleFilter, SaleRepository, SequenceRepository};
use crate::domain::sale::{Sale, SaleItem, SaleStatus};
use crate::domain::value_objects::Money;

use super::database::SqliteUnitOfWork;

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const DAY_FORMAT: &str = "%Y-%m-%d";

const SELECT_SALE: &str = "SELECT id, number, order_id, customer_id, operational_status, \
    order_discount_cents, cancelled_reason, created_at, updated_at, completed_at FROM sales";

const SELECT_ITEM: &str = "SELECT id, product_id, quantity, unit_price_cents, discount_cents, \
    stock_movement_id FROM sale_items";

const SELECT_PAYMENT: &str =
    "SELECT id, sale_id, rental_id, method, amount_cents, paid_at, notes FROM payments";

impl SaleRepository for SqliteUnitOfWork<'_> {
    fn insert_sale(&mut self, sale: &Sale) -> Result<(), RepositoryError> {
        let conn = self.conn();
        let sale_key = sale.id().to_string();

        conn.execute(
            "INSERT INTO sales (id, number, order_id, customer_id, operational_status, \
             order_discount_cents, cancelled_reason, created_at, updated_at, completed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                sale_key,
                sale.number(),
                sale.order_id().to_string(),
                sale.customer_id().to_string(),
                sale.status().as_str(),
                sale.order_discount().cents(),
                sale.cancelled_reason(),
                format_dt(sale.created_at()),
                format_dt(sale.updated_at()),
                sale.completed_at().map(format_dt),
            ],
        )
        .map_err(map_error)?;

        for (position, item) in sale.items().iter().enumerate() {
            conn.execute(
                "INSERT INTO sale_items (id, sale_id, position, product_id, quantity, \
                 unit_price_cents, discount_cents, stock_movement_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    item.id().to_string(),
                    sale_key,
                    position as i64,
                    item.product_id().to_string(),
                    item.quantity(),
                    item.unit_price().cents(),
                    item.discount().cents(),
                    item.stock_movement_id().to_string(),
                ],
            )
            .map_err(map_error)?;
        }
        Ok(())
    }

    fn update_sale(&mut self, sale: &Sale) -> Result<(), RepositoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE sales SET operational_status = ?2, cancelled_reason = ?3, \
                 updated_at = ?4, completed_at = ?5 WHERE id = ?1",
                params![
                    sale.id().to_string(),
                    sale.status().as_str(),
                    sale.cancelled_reason(),
                    format_dt(sale.updated_at()),
                    sale.completed_at().map(format_dt),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_sale(&self, id: Uuid) -> Result<Option<Sale>, RepositoryError> {
        let items = self.load_sale_items(id)?;
        self.conn()
            .query_row(
                &format!("{SELECT_SALE} WHERE id = ?1"),
                params![id.to_string()],
                move |row| row_to_sale(row, items),
            )
            .optional()
            .map_err(map_error)
    }

    fn search_sales(&self, filter: &SaleFilter) -> Result<Vec<Sale>, RepositoryError> {
        let customer = filter.customer_id.map(|id| id.to_string());
        let status = filter.status.map(|status| status.as_str().to_owned());
        let number = filter.number.clone();
        let from = filter.from.map(|day| day.format(DAY_FORMAT).to_string());
        let to = filter.to.map(|day| day.format(DAY_FORMAT).to_string());

        let ids: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM sales \
                     WHERE (?1 IS NULL OR customer_id = ?1) \
                       AND (?2 IS NULL OR operational_status = ?2) \
                       AND (?3 IS NULL OR number = ?3) \
                       AND (?4 IS NULL OR date(created_at) >= ?4) \
                       AND (?5 IS NULL OR date(created_at) <= ?5) \
                     ORDER BY created_at, id",
                )
                .map_err(map_error)?;
            let rows = stmt
                .query_map(params![customer, status, number, from, to], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        let mut sales = Vec::with_capacity(ids.len());
        for raw in ids {
            let id = parse_uuid(&raw).map_err(map_error)?;
            if let Some(sale) = self.find_sale(id)? {
                sales.push(sale);
            }
        }
        Ok(sales)
    }
}

impl SequenceRepository for SqliteUnitOfWork<'_> {
    fn next_sequence_value(&mut self, name: &str) -> Result<i64, RepositoryError> {
        let conn = self.conn();
        let current: i64 = conn
            .query_row(
                "SELECT next_value FROM sequences WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_error)?
            .unwrap_or(1);

        conn.execute(
            "INSERT INTO sequences (name, next_value) VALUES (?1, ?2) \
             ON CONFLICT (name) DO UPDATE SET next_value = excluded.next_value",
            params![name, current + 1],
        )
        .map_err(map_error)?;

        Ok(current)
    }
}

impl PaymentRepository for SqliteUnitOfWork<'_> {
    fn insert_payment(&mut self, payment: &Payment) -> Result<(), RepositoryError> {
        let (sale_id, rental_id) = match payment.target() {
            PaymentTarget::Sale(id) => (Some(id.to_string()), None),
            PaymentTarget::Rental(id) => (None, Some(id.to_string())),
        };

        self.conn()
            .execute(
                "INSERT INTO payments (id, sale_id, rental_id, method, amount_cents, paid_at, notes) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    payment.id().to_string(),
                    sale_id,
                    rental_id,
                    payment.method().as_str(),
                    payment.amount().cents(),
                    format_dt(payment.paid_at()),
                    payment.notes(),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn payments_for_sale(&self, sale_id: Uuid) -> Result<Vec<Payment>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!("{SELECT_PAYMENT} WHERE sale_id = ?1 ORDER BY paid_at, id"))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![sale_id.to_string()], row_to_payment)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }

    fn payments_for_rental(&self, rental_id: Uuid) -> Result<Vec<Payment>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!("{SELECT_PAYMENT} WHERE rental_id = ?1 ORDER BY paid_at, id"))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![rental_id.to_string()], row_to_payment)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

impl SqliteUnitOfWork<'_> {
    fn load_sale_items(&self, sale_id: Uuid) -> Result<Vec<SaleItem>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!("{SELECT_ITEM} WHERE sale_id = ?1 ORDER BY position"))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![sale_id.to_string()], row_to_item)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

fn row_to_sale(row: &Row, items: Vec<SaleItem>) -> rusqlite::Result<Sale> {
    let status_raw: String = row.get("operational_status")?;
    let status = SaleStatus::parse(&status_raw)
        .ok_or_else(|| invalid_value("operational_status", &status_raw))?;
    let completed_at: Option<String> = row.get("completed_at")?;

    Ok(Sale::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        row.get("number")?,
        parse_uuid(&row.get::<_, String>("order_id")?)?,
        parse_uuid(&row.get::<_, String>("customer_id")?)?,
        status,
        Money::from_cents(row.get("order_discount_cents")?),
        items,
        row.get("cancelled_reason")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
        completed_at.as_deref().map(parse_dt).transpose()?,
    ))
}

fn row_to_item(row: &Row) -> rusqlite::Result<SaleItem> {
    Ok(SaleItem::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("product_id")?)?,
        row.get("quantity")?,
        Money::from_cents(row.get("unit_price_cents")?),
        Money::from_cents(row.get("discount_cents")?),
        parse_uuid(&row.get::<_, String>("stock_movement_id")?)?,
    ))
}

fn row_to_payment(row: &Row) -> rusqlite::Result<Payment> {
    let method_raw: String = row.get("method")?;
    let method = PaymentMethod::parse(&method_raw)
        .ok_or_else(|| invalid_value("method", &method_raw))?;

    let sale: Option<String> = row.get("sale_id")?;
    let rental: Option<String> = row.get("rental_id")?;
    let target = match (sale, rental) {
        (Some(sale), None) => PaymentTarget::Sale(parse_uuid(&sale)?),
        (None, Some(rental)) => PaymentTarget::Rental(parse_uuid(&rental)?),
        _ => return Err(invalid_value("sale_id/rental_id", "referência inválida")),
    };

    Ok(Payment::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        target,
        method,
        Money::from_cents(row.get("amount_cents")?),
        parse_dt(&row.get::<_, String>("paid_at")?)?,
        row.get("notes")?,
    ))
}

fn parse_uuid(raw: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(raw)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

fn parse_dt(raw: &str) -> rusqlite::Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(raw, DATE_FORMAT)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

fn format_dt(value: NaiveDateTime) -> String {
    value.format(DATE_FORMAT).to_string()
}

fn invalid_value(column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        format!("valor inválido em {column}: {value}").into(),
    )
}

fn require_one_row(changed: usize) -> Result<(), RepositoryError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(RepositoryError::NotFound)
    }
}

/// Converte erros do SQLite. Mensagens de `RAISE` (triggers) chegam à interface
/// como conflito, com o texto original.
fn map_error(err: rusqlite::Error) -> RepositoryError {
    match err {
        rusqlite::Error::QueryReturnedNoRows => RepositoryError::NotFound,
        rusqlite::Error::SqliteFailure(failure, Some(message))
            if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            if message.starts_with("UNIQUE constraint failed") {
                RepositoryError::Conflict(conflict_message(&message))
            } else if message.starts_with("CHECK constraint failed")
                || message.starts_with("NOT NULL constraint failed")
                || message.starts_with("FOREIGN KEY constraint failed")
            {
                RepositoryError::Storage(message)
            } else {
                RepositoryError::Conflict(message)
            }
        }
        other => RepositoryError::Storage(other.to_string()),
    }
}

fn conflict_message(message: &str) -> String {
    if message.contains("products.sku") {
        "já existe um produto com este SKU".to_owned()
    } else if message.contains("categories.name") {
        "já existe uma categoria com este nome".to_owned()
    } else if message.contains("customers.document") {
        "já existe um cliente com este CPF/CNPJ".to_owned()
    } else if message.contains("sales.order_id") {
        "este pedido já foi convertido em venda".to_owned()
    } else if message.contains("sales.number") {
        "já existe uma venda com este número".to_owned()
    } else {
        "registro duplicado".to_owned()
    }
}