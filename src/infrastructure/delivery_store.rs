use chrono::NaiveDateTime;
use rusqlite::types::Type;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::customer::Address;
use crate::domain::deliverer::{Deliverer, DelivererData};
use crate::domain::delivery::{
    Delivery, DeliveryAssignment, DeliveryHistory, DeliveryStatus, ScheduleEntry, StatusChange,
};
use crate::domain::errors::RepositoryError;
use crate::domain::repositories::{DelivererRepository, DeliveryFilter, DeliveryRepository};
use crate::domain::sale::SaleStatus;
use crate::domain::value_objects::Money;

use super::database::SqliteUnitOfWork;

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

const SELECT_DELIVERER: &str = "SELECT id, name, phone, document, notes, active, created_at, \
    updated_at FROM deliverers";

const SELECT_DELIVERY: &str = "SELECT id, order_id, customer_id, status, destination_street, \
    destination_neighborhood, destination_city, contact_phone, fee_cents, notes, scheduled_for, \
    completed_at, received_by, failure_reason, cancelled_reason, created_at, updated_at \
    FROM deliveries";

const SELECT_ASSIGNMENT: &str = "SELECT id, delivery_id, deliverer_id, assigned_at, unassigned_at \
    FROM delivery_assignments";

impl DelivererRepository for SqliteUnitOfWork<'_> {
    fn insert_deliverer(&mut self, deliverer: &Deliverer) -> Result<(), RepositoryError> {
        let data = deliverer.data();
        self.conn()
            .execute(
                "INSERT INTO deliverers (id, name, phone, document, notes, active, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    deliverer.id().to_string(),
                    data.name,
                    data.phone,
                    data.document,
                    data.notes,
                    deliverer.is_active(),
                    format_dt(deliverer.created_at()),
                    format_dt(deliverer.updated_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_deliverer(&mut self, deliverer: &Deliverer) -> Result<(), RepositoryError> {
        let data = deliverer.data();
        let changed = self
            .conn()
            .execute(
                "UPDATE deliverers SET name = ?2, phone = ?3, document = ?4, notes = ?5, \
                 active = ?6, updated_at = ?7 WHERE id = ?1",
                params![
                    deliverer.id().to_string(),
                    data.name,
                    data.phone,
                    data.document,
                    data.notes,
                    deliverer.is_active(),
                    format_dt(deliverer.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_deliverer(&self, id: Uuid) -> Result<Option<Deliverer>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_DELIVERER} WHERE id = ?1"),
                params![id.to_string()],
                row_to_deliverer,
            )
            .optional()
            .map_err(map_error)
    }

    fn list_deliverers(&self, only_active: bool) -> Result<Vec<Deliverer>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "{SELECT_DELIVERER} WHERE (?1 = 0 OR active = 1) ORDER BY name COLLATE NOCASE"
            ))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![only_active], row_to_deliverer)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

impl DeliveryRepository for SqliteUnitOfWork<'_> {
    fn insert_delivery(&mut self, delivery: &Delivery) -> Result<(), RepositoryError> {
        let destination = delivery.destination();
        self.conn()
            .execute(
                "INSERT INTO deliveries (id, order_id, customer_id, status, destination_street, \
                 destination_neighborhood, destination_city, contact_phone, fee_cents, notes, \
                 scheduled_for, completed_at, received_by, failure_reason, cancelled_reason, \
                 created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    delivery.id().to_string(),
                    delivery.order_id().to_string(),
                    delivery.customer_id().to_string(),
                    delivery.status().as_str(),
                    destination.street(),
                    destination.neighborhood(),
                    destination.city(),
                    delivery.contact_phone(),
                    delivery.fee().cents(),
                    delivery.notes(),
                    delivery.scheduled_for().map(format_dt),
                    delivery.completed_at().map(format_dt),
                    delivery.received_by(),
                    delivery.failure_reason(),
                    delivery.cancelled_reason(),
                    format_dt(delivery.created_at()),
                    format_dt(delivery.updated_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    /// Atualiza apenas os campos que mudam ao longo do ciclo de vida.
    fn update_delivery(&mut self, delivery: &Delivery) -> Result<(), RepositoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE deliveries SET status = ?2, scheduled_for = ?3, completed_at = ?4, \
                 received_by = ?5, failure_reason = ?6, cancelled_reason = ?7, updated_at = ?8 \
                 WHERE id = ?1",
                params![
                    delivery.id().to_string(),
                    delivery.status().as_str(),
                    delivery.scheduled_for().map(format_dt),
                    delivery.completed_at().map(format_dt),
                    delivery.received_by(),
                    delivery.failure_reason(),
                    delivery.cancelled_reason(),
                    format_dt(delivery.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_delivery(&self, id: Uuid) -> Result<Option<Delivery>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_DELIVERY} WHERE id = ?1"),
                params![id.to_string()],
                row_to_delivery,
            )
            .optional()
            .map_err(map_error)
    }

    fn find_delivery_by_order(&self, order_id: Uuid) -> Result<Option<Delivery>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_DELIVERY} WHERE order_id = ?1"),
                params![order_id.to_string()],
                row_to_delivery,
            )
            .optional()
            .map_err(map_error)
    }

    fn search_deliveries(&self, filter: &DeliveryFilter) -> Result<Vec<Delivery>, RepositoryError> {
        let status = filter.status.map(|status| status.as_str().to_owned());
        let customer = filter.customer_id.map(|id| id.to_string());
        let deliverer = filter.deliverer_id.map(|id| id.to_string());
        let from = filter.scheduled_from.map(format_dt);
        let to = filter.scheduled_to.map(format_dt);

        let sql = format!(
            "{SELECT_DELIVERY} \
             WHERE (?1 IS NULL OR status = ?1) \
               AND (?2 IS NULL OR customer_id = ?2) \
               AND (?3 IS NULL OR EXISTS ( \
                    SELECT 1 FROM delivery_assignments a \
                    WHERE a.delivery_id = deliveries.id \
                      AND a.unassigned_at IS NULL \
                      AND a.deliverer_id = ?3)) \
               AND (?4 IS NULL OR scheduled_for >= ?4) \
               AND (?5 IS NULL OR scheduled_for < ?5) \
             ORDER BY scheduled_for IS NULL, scheduled_for, created_at, id"
        );

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(map_error)?;
        let rows = stmt
            .query_map(params![status, customer, deliverer, from, to], row_to_delivery)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }

    fn record_status_change(&mut self, change: &StatusChange) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO delivery_status_history (delivery_id, from_status, to_status, \
                 changed_at, note) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    change.delivery_id.to_string(),
                    change.from.map(DeliveryStatus::as_str),
                    change.to.as_str(),
                    format_dt(change.changed_at),
                    change.note,
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn record_schedule(&mut self, entry: &ScheduleEntry) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO delivery_schedules (delivery_id, scheduled_for, recorded_at) \
                 VALUES (?1, ?2, ?3)",
                params![
                    entry.delivery_id.to_string(),
                    format_dt(entry.scheduled_for),
                    format_dt(entry.recorded_at),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn insert_assignment(&mut self, assignment: &DeliveryAssignment) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO delivery_assignments (id, delivery_id, deliverer_id, assigned_at, \
                 unassigned_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    assignment.id().to_string(),
                    assignment.delivery_id().to_string(),
                    assignment.deliverer_id().to_string(),
                    format_dt(assignment.assigned_at()),
                    assignment.unassigned_at().map(format_dt),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_assignment(&mut self, assignment: &DeliveryAssignment) -> Result<(), RepositoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE delivery_assignments SET unassigned_at = ?2 WHERE id = ?1",
                params![
                    assignment.id().to_string(),
                    assignment.unassigned_at().map(format_dt),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn active_assignment(&self, delivery_id: Uuid) -> Result<Option<DeliveryAssignment>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_ASSIGNMENT} WHERE delivery_id = ?1 AND unassigned_at IS NULL"),
                params![delivery_id.to_string()],
                row_to_assignment,
            )
            .optional()
            .map_err(map_error)
    }

    fn delivery_history(&self, delivery_id: Uuid) -> Result<DeliveryHistory, RepositoryError> {
        let conn = self.conn();
        let key = delivery_id.to_string();

        let status_changes = {
            let mut stmt = conn
                .prepare(
                    "SELECT from_status, to_status, changed_at, note \
                     FROM delivery_status_history WHERE delivery_id = ?1 ORDER BY id",
                )
                .map_err(map_error)?;
            let rows = stmt
                .query_map(params![key], |row| {
                    let from: Option<String> = row.get("from_status")?;
                    let to: String = row.get("to_status")?;
                    Ok(StatusChange {
                        delivery_id,
                        from: from.as_deref().map(status_from).transpose()?,
                        to: status_from(&to)?,
                        changed_at: parse_dt(&row.get::<_, String>("changed_at")?)?,
                        note: row.get("note")?,
                    })
                })
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        let schedules = {
            let mut stmt = conn
                .prepare(
                    "SELECT scheduled_for, recorded_at FROM delivery_schedules \
                     WHERE delivery_id = ?1 ORDER BY id",
                )
                .map_err(map_error)?;
            let rows = stmt
                .query_map(params![key], |row| {
                    Ok(ScheduleEntry {
                        delivery_id,
                        scheduled_for: parse_dt(&row.get::<_, String>("scheduled_for")?)?,
                        recorded_at: parse_dt(&row.get::<_, String>("recorded_at")?)?,
                    })
                })
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        let assignments = {
            let mut stmt = conn
                .prepare(&format!(
                    "{SELECT_ASSIGNMENT} WHERE delivery_id = ?1 ORDER BY assigned_at, id"
                ))
                .map_err(map_error)?;
            let rows = stmt
                .query_map(params![key], row_to_assignment)
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)?
        };

        Ok(DeliveryHistory {
            status_changes,
            schedules,
            assignments,
        })
    }

    fn sale_status_for_order(&self, order_id: Uuid) -> Result<Option<SaleStatus>, RepositoryError> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT operational_status FROM sales WHERE order_id = ?1",
                params![order_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_error)?;

        match raw {
            None => Ok(None),
            Some(raw) => SaleStatus::parse(&raw)
                .map(Some)
                .ok_or_else(|| RepositoryError::Storage(format!("status de venda inválido: {raw}"))),
        }
    }
}

fn row_to_deliverer(row: &Row) -> rusqlite::Result<Deliverer> {
    let data = DelivererData {
        name: row.get("name")?,
        phone: row.get("phone")?,
        document: row.get("document")?,
        notes: row.get("notes")?,
    };
    Ok(Deliverer::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        data,
        row.get("active")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
    ))
}

fn row_to_delivery(row: &Row) -> rusqlite::Result<Delivery> {
    let status_raw: String = row.get("status")?;
    let status = status_from(&status_raw)?;

    let destination = Address::restore(
        row.get("destination_street")?,
        row.get("destination_neighborhood")?,
        row.get("destination_city")?,
    );

    let scheduled: Option<String> = row.get("scheduled_for")?;
    let completed: Option<String> = row.get("completed_at")?;

    Ok(Delivery::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("order_id")?)?,
        parse_uuid(&row.get::<_, String>("customer_id")?)?,
        status,
        destination,
        row.get("contact_phone")?,
        Money::from_cents(row.get("fee_cents")?),
        row.get("notes")?,
        scheduled.as_deref().map(parse_dt).transpose()?,
        completed.as_deref().map(parse_dt).transpose()?,
        row.get("received_by")?,
        row.get("failure_reason")?,
        row.get("cancelled_reason")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
    ))
}

fn row_to_assignment(row: &Row) -> rusqlite::Result<DeliveryAssignment> {
    let unassigned: Option<String> = row.get("unassigned_at")?;
    Ok(DeliveryAssignment::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("delivery_id")?)?,
        parse_uuid(&row.get::<_, String>("deliverer_id")?)?,
        parse_dt(&row.get::<_, String>("assigned_at")?)?,
        unassigned.as_deref().map(parse_dt).transpose()?,
    ))
}

fn status_from(raw: &str) -> rusqlite::Result<DeliveryStatus> {
    DeliveryStatus::parse(raw).ok_or_else(|| invalid_value("status", raw))
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
    if message.contains("deliveries.order_id") {
        "este pedido já possui uma entrega".to_owned()
    } else if message.contains("deliverers.document") {
        "já existe um entregador com este CPF".to_owned()
    } else if message.contains("delivery_assignments") {
        "a entrega já possui um entregador ativo".to_owned()
    } else {
        "registro duplicado".to_owned()
    }
}