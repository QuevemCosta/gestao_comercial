use chrono::NaiveDateTime;
use rusqlite::types::Type;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::audit::AuditEntry;
use crate::domain::errors::RepositoryError;
use crate::domain::rental::{Period, Rental, RentalLine, RentalStatus, ReturnEvent};
use crate::domain::rental_item::{RentalItem, RentalItemData};
use crate::domain::repositories::{
    AuditRepository, RentalFilter, RentalItemRepository, RentalRepository,
};
use crate::domain::value_objects::Money;

use super::database::SqliteUnitOfWork;

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

const SELECT_ITEM: &str = "SELECT id, name, description, category, total_quantity, \
    rental_price_cents, active, notes, created_at, updated_at FROM rental_items";

const SELECT_RENTAL: &str = "SELECT id, number, customer_id, status, pickup_at, \
    expected_return_at, discount_cents, cancelled_reason, picked_up_at, closed_at, \
    created_at, updated_at FROM rentals";

const SELECT_LINE: &str = "SELECT id, rental_item_id, quantity, unit_price_cents, \
    returned_quantity, damaged_quantity, lost_quantity FROM rental_lines";

impl RentalItemRepository for SqliteUnitOfWork<'_> {
    fn insert_rental_item(&mut self, item: &RentalItem) -> Result<(), RepositoryError> {
        let data = item.data();
        self.conn()
            .execute(
                "INSERT INTO rental_items (id, name, description, category, total_quantity, \
                 rental_price_cents, active, notes, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    item.id().to_string(),
                    data.name,
                    data.description,
                    data.category,
                    item.total_quantity(),
                    data.rental_price.cents(),
                    item.is_active(),
                    data.notes,
                    format_dt(item.created_at()),
                    format_dt(item.updated_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_rental_item(&mut self, item: &RentalItem) -> Result<(), RepositoryError> {
        let data = item.data();
        let changed = self
            .conn()
            .execute(
                "UPDATE rental_items SET name = ?2, description = ?3, category = ?4, \
                 total_quantity = ?5, rental_price_cents = ?6, active = ?7, notes = ?8, \
                 updated_at = ?9 WHERE id = ?1",
                params![
                    item.id().to_string(),
                    data.name,
                    data.description,
                    data.category,
                    item.total_quantity(),
                    data.rental_price.cents(),
                    item.is_active(),
                    data.notes,
                    format_dt(item.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_rental_item(&self, id: Uuid) -> Result<Option<RentalItem>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_ITEM} WHERE id = ?1"),
                params![id.to_string()],
                row_to_item,
            )
            .optional()
            .map_err(map_error)
    }

    fn list_rental_items(&self, only_active: bool) -> Result<Vec<RentalItem>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "{SELECT_ITEM} WHERE (?1 = 0 OR active = 1) ORDER BY name COLLATE NOCASE"
            ))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![only_active], row_to_item)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

impl RentalRepository for SqliteUnitOfWork<'_> {
    fn insert_rental(&mut self, rental: &Rental) -> Result<(), RepositoryError> {
        let conn = self.conn();
        let rental_key = rental.id().to_string();
        let period = rental.period();

        conn.execute(
            "INSERT INTO rentals (id, number, customer_id, status, pickup_at, expected_return_at, \
             discount_cents, cancelled_reason, picked_up_at, closed_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                rental_key,
                rental.number(),
                rental.customer_id().to_string(),
                rental.status().as_str(),
                format_dt(period.start()),
                format_dt(period.end()),
                rental.discount().cents(),
                rental.cancelled_reason(),
                rental.picked_up_at().map(format_dt),
                rental.closed_at().map(format_dt),
                format_dt(rental.created_at()),
                format_dt(rental.updated_at()),
            ],
        )
        .map_err(map_error)?;

        for (position, line) in rental.lines().iter().enumerate() {
            conn.execute(
                "INSERT INTO rental_lines (id, rental_id, position, rental_item_id, quantity, \
                 unit_price_cents, returned_quantity, damaged_quantity, lost_quantity) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    line.id().to_string(),
                    rental_key,
                    position as i64,
                    line.rental_item_id().to_string(),
                    line.quantity(),
                    line.unit_price().cents(),
                    line.returned(),
                    line.damaged(),
                    line.lost(),
                ],
            )
            .map_err(map_error)?;
        }
        Ok(())
    }

    fn update_rental(&mut self, rental: &Rental) -> Result<(), RepositoryError> {
        let conn = self.conn();
        let changed = conn
            .execute(
                "UPDATE rentals SET status = ?2, cancelled_reason = ?3, picked_up_at = ?4, \
                 closed_at = ?5, updated_at = ?6 WHERE id = ?1",
                params![
                    rental.id().to_string(),
                    rental.status().as_str(),
                    rental.cancelled_reason(),
                    rental.picked_up_at().map(format_dt),
                    rental.closed_at().map(format_dt),
                    format_dt(rental.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)?;

        for line in rental.lines() {
            conn.execute(
                "UPDATE rental_lines SET returned_quantity = ?2, damaged_quantity = ?3, \
                 lost_quantity = ?4 WHERE id = ?1",
                params![
                    line.id().to_string(),
                    line.returned(),
                    line.damaged(),
                    line.lost(),
                ],
            )
            .map_err(map_error)?;
        }
        Ok(())
    }

    fn find_rental(&self, id: Uuid) -> Result<Option<Rental>, RepositoryError> {
        let lines = self.load_lines(id)?;
        self.conn()
            .query_row(
                &format!("{SELECT_RENTAL} WHERE id = ?1"),
                params![id.to_string()],
                move |row| row_to_rental(row, lines),
            )
            .optional()
            .map_err(map_error)
    }

    fn search_rentals(&self, filter: &RentalFilter) -> Result<Vec<Rental>, RepositoryError> {
        let customer = filter.customer_id.map(|id| id.to_string());
        let status = filter.status.map(|status| status.as_str().to_owned());

        let ids: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM rentals \
                     WHERE (?1 IS NULL OR customer_id = ?1) AND (?2 IS NULL OR status = ?2) \
                     ORDER BY created_at, id",
                )
                .map_err(map_error)?;
            let rows = stmt
                .query_map(params![customer, status], |row| row.get::<_, String>(0))
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        self.load_rentals(ids)
    }

    fn open_rentals(&self) -> Result<Vec<Rental>, RepositoryError> {
        let ids: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM rentals WHERE status IN ('reserved', 'active') \
                     ORDER BY pickup_at, id",
                )
                .map_err(map_error)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        self.load_rentals(ids)
    }

    fn insert_return_events(&mut self, events: &[ReturnEvent]) -> Result<(), RepositoryError> {
        let conn = self.conn();
        for event in events {
            conn.execute(
                "INSERT INTO rental_return_events (id, rental_line_id, occurred_at, \
                 returned_quantity, damaged_quantity, lost_quantity, notes) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    event.id.to_string(),
                    event.line_id.to_string(),
                    format_dt(event.occurred_at),
                    event.count.returned,
                    event.count.damaged,
                    event.count.lost,
                    event.notes,
                ],
            )
            .map_err(map_error)?;
        }
        Ok(())
    }
}

impl AuditRepository for SqliteUnitOfWork<'_> {
    fn record_audit(&mut self, entry: &AuditEntry) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO audit_log (occurred_at, action, entity, entity_id, details) \
                 VALUES (?1, ?2, ?3, NULL, ?4)",
                params![
                    format_dt(entry.occurred_at),
                    entry.action,
                    entry.entity,
                    entry.details,
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }
}

impl SqliteUnitOfWork<'_> {
    fn load_lines(&self, rental_id: Uuid) -> Result<Vec<RentalLine>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!("{SELECT_LINE} WHERE rental_id = ?1 ORDER BY position"))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![rental_id.to_string()], row_to_line)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }

    fn load_rentals(&self, ids: Vec<String>) -> Result<Vec<Rental>, RepositoryError> {
        let mut rentals = Vec::with_capacity(ids.len());
        for raw in ids {
            let id = parse_uuid(&raw).map_err(map_error)?;
            if let Some(rental) = self.find_rental(id)? {
                rentals.push(rental);
            }
        }
        Ok(rentals)
    }
}

fn row_to_item(row: &Row) -> rusqlite::Result<RentalItem> {
    let data = RentalItemData {
        name: row.get("name")?,
        description: row.get("description")?,
        category: row.get("category")?,
        rental_price: Money::from_cents(row.get("rental_price_cents")?),
        notes: row.get("notes")?,
    };
    Ok(RentalItem::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        data,
        row.get("total_quantity")?,
        row.get("active")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
    ))
}

fn row_to_line(row: &Row) -> rusqlite::Result<RentalLine> {
    Ok(RentalLine::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("rental_item_id")?)?,
        row.get("quantity")?,
        Money::from_cents(row.get("unit_price_cents")?),
        row.get("returned_quantity")?,
        row.get("damaged_quantity")?,
        row.get("lost_quantity")?,
    ))
}

fn row_to_rental(row: &Row, lines: Vec<RentalLine>) -> rusqlite::Result<Rental> {
    let status_raw: String = row.get("status")?;
    let status = RentalStatus::parse(&status_raw)
        .ok_or_else(|| invalid_value("status", &status_raw))?;

    let period = Period::restore(
        parse_dt(&row.get::<_, String>("pickup_at")?)?,
        parse_dt(&row.get::<_, String>("expected_return_at")?)?,
    );
    let picked_up: Option<String> = row.get("picked_up_at")?;
    let closed: Option<String> = row.get("closed_at")?;

    Ok(Rental::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        row.get("number")?,
        parse_uuid(&row.get::<_, String>("customer_id")?)?,
        status,
        period,
        Money::from_cents(row.get("discount_cents")?),
        lines,
        row.get("cancelled_reason")?,
        picked_up.as_deref().map(parse_dt).transpose()?,
        closed.as_deref().map(parse_dt).transpose()?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
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
    if message.contains("rentals.number") {
        "já existe uma locação com este número".to_owned()
    } else {
        "registro duplicado".to_owned()
    }
}