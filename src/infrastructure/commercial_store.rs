use chrono::NaiveDateTime;
use rusqlite::types::Type;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::customer::{Address, Customer, CustomerData, PersonType};
use crate::domain::errors::RepositoryError;
use crate::domain::order::{Order, OrderItem, OrderOrigin, OrderStatus, ReceiveMode};
use crate::domain::repositories::{CustomerFilter, CustomerRepository, OrderFilter, OrderRepository};
use crate::domain::text::search_key;
use crate::domain::value_objects::Money;

use super::database::SqliteUnitOfWork;

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

const SELECT_CUSTOMER: &str = "SELECT id, person_type, name, document, phone, whatsapp, \
    street, neighborhood, city, notes, active, created_at, updated_at FROM customers";

const SELECT_ORDER: &str = "SELECT id, customer_id, status, origin, receive_mode, \
    delivery_street, delivery_neighborhood, delivery_city, order_discount_cents, notes, \
    created_at, updated_at FROM orders";

impl CustomerRepository for SqliteUnitOfWork<'_> {
    fn insert_customer(&mut self, customer: &Customer) -> Result<(), RepositoryError> {
        let data = customer.data();
        let (street, neighborhood, city) = address_columns(data.address.as_ref());
        self.conn()
            .execute(
                "INSERT INTO customers (id, person_type, name, document, phone, whatsapp, \
                 street, neighborhood, city, notes, active, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    customer.id().to_string(),
                    data.person_type.as_str(),
                    data.name,
                    data.document,
                    data.phone,
                    data.whatsapp,
                    street,
                    neighborhood,
                    city,
                    data.notes,
                    customer.is_active(),
                    format_dt(customer.created_at()),
                    format_dt(customer.updated_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_customer(&mut self, customer: &Customer) -> Result<(), RepositoryError> {
        let data = customer.data();
        let (street, neighborhood, city) = address_columns(data.address.as_ref());
        let changed = self
            .conn()
            .execute(
                "UPDATE customers SET person_type = ?2, name = ?3, document = ?4, phone = ?5, \
                 whatsapp = ?6, street = ?7, neighborhood = ?8, city = ?9, notes = ?10, \
                 active = ?11, updated_at = ?12 WHERE id = ?1",
                params![
                    customer.id().to_string(),
                    data.person_type.as_str(),
                    data.name,
                    data.document,
                    data.phone,
                    data.whatsapp,
                    street,
                    neighborhood,
                    city,
                    data.notes,
                    customer.is_active(),
                    format_dt(customer.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_customer(&self, id: Uuid) -> Result<Option<Customer>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_CUSTOMER} WHERE id = ?1"),
                params![id.to_string()],
                row_to_customer,
            )
            .optional()
            .map_err(map_error)
    }

    fn search_customers(&self, filter: &CustomerFilter) -> Result<Vec<Customer>, RepositoryError> {
        let text = filter.text.as_deref().map(str::trim).filter(|value| !value.is_empty());
        // Texto vira padrão de nome (sem acento) e, se contiver dígitos, padrão de documento/telefone.
        let name_pattern = text.map(|value| format!("%{}%", escape_like(&search_key(value))));
        let digits: String = text
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        let digits_pattern = (!digits.is_empty()).then(|| format!("%{digits}%"));

        let sql = format!(
            "{SELECT_CUSTOMER} \
             WHERE (?1 = 1 OR active = 1) \
               AND (?2 IS NULL \
                    OR search_key(name) LIKE ?2 ESCAPE '\\' \
                    OR (?3 IS NOT NULL AND (document LIKE ?3 ESCAPE '\\' \
                                            OR phone LIKE ?3 ESCAPE '\\' \
                                            OR whatsapp LIKE ?3 ESCAPE '\\'))) \
             ORDER BY name COLLATE NOCASE"
        );

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(map_error)?;
        let rows = stmt
            .query_map(
                params![filter.include_inactive, name_pattern, digits_pattern],
                row_to_customer,
            )
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

impl OrderRepository for SqliteUnitOfWork<'_> {
    fn insert_order(&mut self, order: &Order) -> Result<(), RepositoryError> {
        let (street, neighborhood, city) = address_columns(order.delivery_address());
        self.conn()
            .execute(
                "INSERT INTO orders (id, customer_id, status, origin, receive_mode, \
                 delivery_street, delivery_neighborhood, delivery_city, order_discount_cents, \
                 notes, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    order.id().to_string(),
                    order.customer_id().to_string(),
                    order.status().as_str(),
                    order.origin().as_str(),
                    order.receive_mode().as_str(),
                    street,
                    neighborhood,
                    city,
                    order.order_discount().cents(),
                    order.notes(),
                    format_dt(order.created_at()),
                    format_dt(order.updated_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_order(&mut self, order: &Order) -> Result<(), RepositoryError> {
        let (street, neighborhood, city) = address_columns(order.delivery_address());
        let changed = self
            .conn()
            .execute(
                "UPDATE orders SET status = ?2, origin = ?3, receive_mode = ?4, \
                 delivery_street = ?5, delivery_neighborhood = ?6, delivery_city = ?7, \
                 order_discount_cents = ?8, notes = ?9, updated_at = ?10 WHERE id = ?1",
                params![
                    order.id().to_string(),
                    order.status().as_str(),
                    order.origin().as_str(),
                    order.receive_mode().as_str(),
                    street,
                    neighborhood,
                    city,
                    order.order_discount().cents(),
                    order.notes(),
                    format_dt(order.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn replace_order_items(&mut self, order: &Order) -> Result<(), RepositoryError> {
        let order_key = order.id().to_string();
        let conn = self.conn();

        // Os triggers impedem alterações quando o pedido não está em elaboração.
        conn.execute("DELETE FROM order_items WHERE order_id = ?1", params![order_key])
            .map_err(map_error)?;

        for (position, item) in order.items().iter().enumerate() {
            conn.execute(
                "INSERT INTO order_items (id, order_id, position, product_id, quantity, \
                 unit_price_cents, discount_cents) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    item.id().to_string(),
                    order_key,
                    position as i64,
                    item.product_id().to_string(),
                    item.quantity(),
                    item.unit_price().cents(),
                    item.discount().cents(),
                ],
            )
            .map_err(map_error)?;
        }
        Ok(())
    }

    fn find_order(&self, id: Uuid) -> Result<Option<Order>, RepositoryError> {
        let items = self.load_items(id)?;
        self.conn()
            .query_row(
                &format!("{SELECT_ORDER} WHERE id = ?1"),
                params![id.to_string()],
                move |row| row_to_order(row, items),
            )
            .optional()
            .map_err(map_error)
    }

    fn search_orders(&self, filter: &OrderFilter) -> Result<Vec<Order>, RepositoryError> {
        let customer = filter.customer_id.map(|id| id.to_string());
        let status = filter.status.map(|status| status.as_str().to_owned());

        let ids: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM orders \
                     WHERE (?1 IS NULL OR customer_id = ?1) AND (?2 IS NULL OR status = ?2) \
                     ORDER BY created_at, id",
                )
                .map_err(map_error)?;
            let rows = stmt
                .query_map(params![customer, status], |row| row.get::<_, String>(0))
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        let mut orders = Vec::with_capacity(ids.len());
        for raw in ids {
            let id = parse_uuid(&raw).map_err(map_error)?;
            if let Some(order) = self.find_order(id)? {
                orders.push(order);
            }
        }
        Ok(orders)
    }
}

impl SqliteUnitOfWork<'_> {
    fn load_items(&self, order_id: Uuid) -> Result<Vec<OrderItem>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, product_id, quantity, unit_price_cents, discount_cents \
                 FROM order_items WHERE order_id = ?1 ORDER BY position",
            )
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![order_id.to_string()], row_to_item)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

fn row_to_customer(row: &Row) -> rusqlite::Result<Customer> {
    let person_type_raw: String = row.get("person_type")?;
    let person_type = PersonType::parse(&person_type_raw)
        .ok_or_else(|| invalid_value("person_type", &person_type_raw))?;

    let street: Option<String> = row.get("street")?;
    let neighborhood: Option<String> = row.get("neighborhood")?;
    let city: Option<String> = row.get("city")?;
    let address = match (street, neighborhood, city) {
        (Some(street), Some(neighborhood), Some(city)) => {
            Some(Address::restore(street, neighborhood, city))
        }
        _ => None,
    };

    let data = CustomerData {
        person_type,
        name: row.get("name")?,
        document: row.get("document")?,
        phone: row.get("phone")?,
        whatsapp: row.get("whatsapp")?,
        address,
        notes: row.get("notes")?,
    };

    Ok(Customer::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        data,
        row.get("active")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
    ))
}

fn row_to_order(row: &Row, items: Vec<OrderItem>) -> rusqlite::Result<Order> {
    let status_raw: String = row.get("status")?;
    let status = OrderStatus::parse(&status_raw)
        .ok_or_else(|| invalid_value("status", &status_raw))?;

    let origin_raw: String = row.get("origin")?;
    let origin = OrderOrigin::parse(&origin_raw)
        .ok_or_else(|| invalid_value("origin", &origin_raw))?;

    let mode_raw: String = row.get("receive_mode")?;
    let receive_mode = ReceiveMode::parse(&mode_raw)
        .ok_or_else(|| invalid_value("receive_mode", &mode_raw))?;

    let street: Option<String> = row.get("delivery_street")?;
    let neighborhood: Option<String> = row.get("delivery_neighborhood")?;
    let city: Option<String> = row.get("delivery_city")?;
    let delivery_address = match (street, neighborhood, city) {
        (Some(street), Some(neighborhood), Some(city)) => {
            Some(Address::restore(street, neighborhood, city))
        }
        _ => None,
    };

    Ok(Order::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("customer_id")?)?,
        status,
        origin,
        receive_mode,
        delivery_address,
        Money::from_cents(row.get("order_discount_cents")?),
        row.get("notes")?,
        items,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
    ))
}

fn row_to_item(row: &Row) -> rusqlite::Result<OrderItem> {
    Ok(OrderItem::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("product_id")?)?,
        row.get("quantity")?,
        Money::from_cents(row.get("unit_price_cents")?),
        Money::from_cents(row.get("discount_cents")?),
    ))
}

fn address_columns(address: Option<&Address>) -> (Option<String>, Option<String>, Option<String>) {
    match address {
        Some(address) => (
            Some(address.street().to_owned()),
            Some(address.neighborhood().to_owned()),
            Some(address.city().to_owned()),
        ),
        None => (None, None, None),
    }
}

/// Escapa os caracteres especiais do LIKE para que sejam tratados como texto comum.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
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
/// como conflito, com o texto original, que já é compreensível.
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
    } else {
        "registro duplicado".to_owned()
    }
}