use chrono::NaiveDateTime;
use rusqlite::types::Type;
use rusqlite::{params, Row};
use uuid::Uuid;

use crate::domain::errors::RepositoryError;
use crate::domain::payment::{Payment, PaymentMethod, PaymentTarget};
use crate::domain::report_repository::ReportRepository;
use crate::domain::value_objects::Money;

use super::database::SqliteUnitOfWork;

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

impl ReportRepository for SqliteUnitOfWork<'_> {
    fn payments_between(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Payment>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, sale_id, rental_id, method, amount_cents, paid_at, notes \
                 FROM payments WHERE paid_at >= ?1 AND paid_at < ?2 ORDER BY paid_at, id",
            )
            .map_err(map_error)?;
        let rows = stmt
            .query_map(
                params![
                    from.format(DATE_FORMAT).to_string(),
                    to.format(DATE_FORMAT).to_string()
                ],
                row_to_payment,
            )
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
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

fn invalid_value(column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        format!("valor inválido em {column}: {value}").into(),
    )
}

fn map_error(err: rusqlite::Error) -> RepositoryError {
    RepositoryError::Storage(err.to_string())
}