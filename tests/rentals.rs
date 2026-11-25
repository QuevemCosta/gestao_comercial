use std::cell::Cell;
use std::error::Error;

use chrono::NaiveDateTime;
use uuid::Uuid;

use gestao_comercial::application::clock::Clock;
use gestao_comercial::application::customers::Customers;
use gestao_comercial::application::error::AppError;
use gestao_comercial::application::rentals::{RentalLineInput, Rentals, ReturnLineInput};
use gestao_comercial::domain::customer::{CustomerData, PersonType};
use gestao_comercial::domain::errors::DomainError;
use gestao_comercial::domain::payment::{PaymentMethod, PaymentStatus};
use gestao_comercial::domain::rental::{Rental, RentalStatus};
use gestao_comercial::domain::rental_item::RentalItemData;
use gestao_comercial::domain::value_objects::Money;
use gestao_comercial::infrastructure::database::Database;

use chrono::NaiveDate;

type TestResult = Result<(), Box<dyn Error>>;

/// Relógio controlável pelo teste: permite simular o avanço do tempo.
struct MovableClock(Cell<NaiveDateTime>);

impl MovableClock {
    fn new(now: NaiveDateTime) -> Self {
        Self(Cell::new(now))
    }

    fn set(&self, now: NaiveDateTime) {
        self.0.set(now);
    }
}

impl Clock for MovableClock {
    fn now(&self) -> NaiveDateTime {
        self.0.get()
    }
}

/// Data e hora em novembro de 2026.
fn at(day: u32, hour: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, day)
        .and_then(|date| date.and_hms_opt(hour, 0, 0))
        .expect("data de teste válida")
}

fn customer(db: &mut Database, clock: &MovableClock) -> Result<Uuid, AppError> {
    let created = Customers::new(&mut *db, clock).register(CustomerData {
        person_type: PersonType::Individual,
        name: "Ana Paula Lima".to_owned(),
        document: None,
        phone: Some("11912345678".to_owned()),
        whatsapp: None,
        address: None,
        notes: String::new(),
    })?;
    Ok(created.id())
}

fn item(db: &mut Database, clock: &MovableClock, name: &str, total: i64) -> Result<Uuid, AppError> {
    let created = Rentals::new(&mut *db, clock).register_item(
        RentalItemData {
            name: name.to_owned(),
            description: String::new(),
            category: "Mobiliário".to_owned(),
            rental_price: Money::from_cents(500),
            notes: String::new(),
        },
        total,
    )?;
    Ok(created.id())
}

fn line(item_id: Uuid, quantity: i64) -> RentalLineInput {
    RentalLineInput {
        rental_item_id: item_id,
        quantity,
        unit_price: Money::from_cents(500),
    }
}

fn ret(line_id: Uuid, returned: i64, damaged: i64, lost: i64) -> ReturnLineInput {
    ReturnLineInput {
        line_id,
        returned,
        damaged,
        lost,
        notes: String::new(),
    }
}

fn rent(
    db: &mut Database,
    clock: &MovableClock,
    customer_id: Uuid,
    pickup: NaiveDateTime,
    expected: NaiveDateTime,
    lines: Vec<RentalLineInput>,
) -> Result<Rental, AppError> {
    Rentals::new(&mut *db, clock).create(customer_id, pickup, expected, Money::ZERO, lines)
}

#[test]
fn overlapping_rentals_share_the_same_capacity() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 100)?;

    rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 30)])?;
    rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 20)])?;

    let available = Rentals::new(&mut db, &clock).availability(cadeira, at(21, 8), at(21, 20))?;
    assert_eq!(available, 50);

    let err = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 51)]).unwrap_err();
    assert_eq!(
        err,
        AppError::Validation(DomainError::InsufficientAvailability {
            available: 50,
            requested: 51,
        })
    );

    rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 50)])?;
    Ok(())
}

#[test]
fn adjacent_periods_do_not_conflict() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 10)])?;
    rent(&mut db, &clock, ana, at(21, 20), at(22, 8), vec![line(cadeira, 10)])?;
    Ok(())
}

#[test]
fn full_lifecycle_closes_rental_after_all_units_are_accounted() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 5)])?;
    assert_eq!(rental.status(), RentalStatus::Reserved);
    let line_id = rental.lines()[0].id();

    clock.set(at(21, 8));
    let picked = Rentals::new(&mut db, &clock).pick_up(rental.id())?;
    assert_eq!(picked.status(), RentalStatus::Active);

    clock.set(at(21, 19));
    let partial = Rentals::new(&mut db, &clock).register_return(rental.id(), vec![ret(line_id, 2, 0, 0)])?;
    assert_eq!(partial.status(), RentalStatus::Active);

    let closed = Rentals::new(&mut db, &clock).register_return(rental.id(), vec![ret(line_id, 3, 0, 0)])?;
    assert_eq!(closed.status(), RentalStatus::Closed);
    assert_eq!(closed.closed_at(), Some(at(21, 19)));
    Ok(())
}

#[test]
fn return_beyond_outstanding_units_is_rejected_without_side_effects() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 5)])?;
    let line_id = rental.lines()[0].id();
    clock.set(at(21, 8));
    Rentals::new(&mut db, &clock).pick_up(rental.id())?;

    let err = Rentals::new(&mut db, &clock)
        .register_return(rental.id(), vec![ret(line_id, 6, 0, 0)])
        .unwrap_err();
    assert_eq!(
        err,
        AppError::Validation(DomainError::ReturnExceedsOutstanding { outstanding: 5 })
    );

    let summary = Rentals::new(&mut db, &clock).summary(rental.id())?;
    assert_eq!(summary.outstanding_units, 5);
    assert_eq!(summary.rental.status(), RentalStatus::Active);
    Ok(())
}

#[test]
fn damaged_and_lost_units_are_recorded_and_close_the_rental() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 5)])?;
    let line_id = rental.lines()[0].id();
    clock.set(at(21, 8));
    Rentals::new(&mut db, &clock).pick_up(rental.id())?;

    clock.set(at(21, 19));
    let closed = Rentals::new(&mut db, &clock)
        .register_return(rental.id(), vec![ret(line_id, 2, 1, 2)])?;

    assert_eq!(closed.status(), RentalStatus::Closed);
    assert_eq!(closed.lines()[0].returned(), 2);
    assert_eq!(closed.lines()[0].damaged(), 1);
    assert_eq!(closed.lines()[0].lost(), 2);
    Ok(())
}

#[test]
fn cancelling_reserved_rental_releases_capacity_and_active_cannot_be_cancelled() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let first = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 10)])?;
    let blocked = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 1)]).unwrap_err();
    assert_eq!(
        blocked,
        AppError::Validation(DomainError::InsufficientAvailability {
            available: 0,
            requested: 1,
        })
    );

    Rentals::new(&mut db, &clock).cancel(first.id(), "cliente desistiu")?;
    rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 1)])?;

    let second = rent(&mut db, &clock, ana, at(23, 8), at(23, 20), vec![line(cadeira, 1)])?;
    Rentals::new(&mut db, &clock).pick_up(second.id())?;
    let err = Rentals::new(&mut db, &clock)
        .cancel(second.id(), "tarde demais")
        .unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::RentalNotCancellable));
    Ok(())
}

#[test]
fn late_rental_keeps_item_booked_until_returned() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 5)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 5)])?;
    let line_id = rental.lines()[0].id();
    clock.set(at(21, 8));
    Rentals::new(&mut db, &clock).pick_up(rental.id())?;

    clock.set(at(22, 10));
    let summary = Rentals::new(&mut db, &clock).summary(rental.id())?;
    assert!(summary.late);

    let available = Rentals::new(&mut db, &clock).availability(cadeira, at(22, 12), at(22, 18))?;
    assert_eq!(available, 0);

    clock.set(at(22, 11));
    Rentals::new(&mut db, &clock).register_return(rental.id(), vec![ret(line_id, 5, 0, 0)])?;
    let available_after = Rentals::new(&mut db, &clock).availability(cadeira, at(22, 12), at(22, 18))?;
    assert_eq!(available_after, 5);
    Ok(())
}

#[test]
fn rental_payments_follow_the_balance() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 3)])?;

    Rentals::new(&mut db, &clock).register_payment(rental.id(), PaymentMethod::Pix, Money::from_cents(500), "sinal")?;
    let partial = Rentals::new(&mut db, &clock).summary(rental.id())?;
    assert_eq!(partial.total, Money::from_cents(1500));
    assert_eq!(partial.payment_status, PaymentStatus::Partial);

    Rentals::new(&mut db, &clock).register_payment(rental.id(), PaymentMethod::Cash, Money::from_cents(1000), "")?;
    let paid = Rentals::new(&mut db, &clock).summary(rental.id())?;
    assert_eq!(paid.payment_status, PaymentStatus::Paid);
    assert_eq!(paid.balance, Money::ZERO);

    let over = Rentals::new(&mut db, &clock)
        .register_payment(rental.id(), PaymentMethod::Cash, Money::from_cents(1), "")
        .unwrap_err();
    assert!(matches!(
        over,
        AppError::Validation(DomainError::PaymentExceedsBalance { .. })
    ));
    Ok(())
}

#[test]
fn cancelled_rental_rejects_payments() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 1)])?;
    Rentals::new(&mut db, &clock).cancel(rental.id(), "erro")?;

    let err = Rentals::new(&mut db, &clock)
        .register_payment(rental.id(), PaymentMethod::Cash, Money::from_cents(100), "")
        .unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::RentalCancelled));
    Ok(())
}

#[test]
fn write_off_is_blocked_by_reservations_and_is_audited() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 6)])?;

    let blocked = Rentals::new(&mut db, &clock).write_off(cadeira, 5, "perda").unwrap_err();
    assert_eq!(
        blocked,
        AppError::Validation(DomainError::WriteOffBlockedByReservations { required: 6 })
    );
    assert_eq!(Rentals::new(&mut db, &clock).get_item(cadeira)?.total_quantity(), 10);

    let count_before: i64 = db.connection().query_row(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'baixa_locacao'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count_before, 0);

    let written = Rentals::new(&mut db, &clock).write_off(cadeira, 4, "perda")?;
    assert_eq!(written.total_quantity(), 6);

    let count_after: i64 = db.connection().query_row(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'baixa_locacao'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count_after, 1);
    Ok(())
}

#[test]
fn inactive_item_cannot_be_rented() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;
    Rentals::new(&mut db, &clock).set_item_active(cadeira, false)?;

    let err = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 1)]).unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::RentalItemInactive));
    Ok(())
}

#[test]
fn invalid_period_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let err = rent(&mut db, &clock, ana, at(21, 8), at(21, 8), vec![line(cadeira, 1)]).unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::InvalidPeriod));
    Ok(())
}

#[test]
fn return_events_and_rental_payments_are_immutable() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let ana = customer(&mut db, &clock)?;
    let cadeira = item(&mut db, &clock, "Cadeira", 10)?;

    let rental = rent(&mut db, &clock, ana, at(21, 8), at(21, 20), vec![line(cadeira, 2)])?;
    let line_id = rental.lines()[0].id();
    clock.set(at(21, 8));
    Rentals::new(&mut db, &clock).pick_up(rental.id())?;
    Rentals::new(&mut db, &clock).register_return(rental.id(), vec![ret(line_id, 1, 0, 0)])?;
    Rentals::new(&mut db, &clock).register_payment(rental.id(), PaymentMethod::Cash, Money::from_cents(100), "")?;

    let conn = db.connection();
    assert!(conn.execute("UPDATE rental_return_events SET notes = 'alterado'", []).is_err());
    assert!(conn.execute("DELETE FROM rental_return_events", []).is_err());
    assert!(conn.execute("UPDATE payments SET amount_cents = 1", []).is_err());
    assert!(conn.execute("DELETE FROM payments", []).is_err());
    Ok(())
}