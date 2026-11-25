use std::cell::Cell;
use std::error::Error;

use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use gestao_comercial::application::catalog::Catalog;
use gestao_comercial::application::clock::Clock;
use gestao_comercial::application::customers::Customers;
use gestao_comercial::application::deliveries::{Deliveries, NewDeliveryInput};
use gestao_comercial::application::error::AppError;
use gestao_comercial::application::orders::{OrderItemInput, Orders};
use gestao_comercial::application::sales::Sales;
use gestao_comercial::application::stock::StockControl;
use gestao_comercial::domain::customer::{Address, CustomerData, PersonType};
use gestao_comercial::domain::deliverer::DelivererData;
use gestao_comercial::domain::delivery::{Delivery, DeliveryStatus};
use gestao_comercial::domain::errors::DomainError;
use gestao_comercial::domain::order::{OrderOrigin, ReceiveMode};
use gestao_comercial::domain::payment::PaymentStatus;
use gestao_comercial::domain::product::ProductData;
use gestao_comercial::domain::repositories::DeliveryFilter;
use gestao_comercial::domain::sale::SaleStatus;
use gestao_comercial::domain::value_objects::Money;
use gestao_comercial::infrastructure::database::Database;

type TestResult = Result<(), Box<dyn Error>>;

const CPF_ANA: &str = "529.982.247-25";
const CPF_CARLOS: &str = "11144477735";

/// Relógio controlável pelo teste.
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

fn at(day: u32, hour: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, day)
        .and_then(|date| date.and_hms_opt(hour, 0, 0))
        .expect("data de teste válida")
}

/// Cria um pedido confirmado. Com `delivery = true`, o pedido tem modalidade de entrega.
fn setup_order(
    db: &mut Database,
    clock: &MovableClock,
    sku: &str,
    delivery: bool,
) -> Result<(Uuid, Uuid), Box<dyn Error>> {
    let customer = Customers::new(&mut *db, clock).register(CustomerData {
        person_type: PersonType::Individual,
        name: format!("Cliente {sku}"),
        document: None,
        phone: Some("11912345678".to_owned()),
        whatsapp: None,
        address: None,
        notes: String::new(),
    })?;

    let product = Catalog::new(&mut *db, clock).create_product(ProductData {
        sku: sku.to_owned(),
        name: "Cadeira".to_owned(),
        description: String::new(),
        category_id: None,
        purchase_price: Money::from_cents(1_000),
        sale_price: Money::from_cents(2_500),
        min_stock: 0,
        unit: "un".to_owned(),
    })?;
    StockControl::new(&mut *db, clock).register_entry(product.id(), 20, "carga inicial")?;

    let (mode, address) = if delivery {
        (
            ReceiveMode::Delivery,
            Some(Address::new("Rua das Flores, 10", "Centro", "Fortaleza")?),
        )
    } else {
        (ReceiveMode::Pickup, None)
    };

    let order = Orders::new(&mut *db, clock).create(customer.id(), OrderOrigin::Whatsapp, mode, address)?;
    let order = Orders::new(&mut *db, clock).set_items(
        order.id(),
        vec![OrderItemInput {
            product_id: product.id(),
            quantity: 2,
            unit_price: Money::from_cents(2_500),
            discount: Money::ZERO,
        }],
    )?;
    let sale = Sales::new(&mut *db, clock).confirm_order(order.id())?;
    Ok((order.id(), sale.id()))
}

fn register_deliverer(
    db: &mut Database,
    clock: &MovableClock,
    name: &str,
    document: &str,
) -> Result<Uuid, AppError> {
    let deliverer = Deliveries::new(&mut *db, clock).register_deliverer(DelivererData {
        name: name.to_owned(),
        phone: "85988887777".to_owned(),
        document: Some(document.to_owned()),
        notes: String::new(),
    })?;
    Ok(deliverer.id())
}

fn create_delivery(
    db: &mut Database,
    clock: &MovableClock,
    order_id: Uuid,
    scheduled_for: Option<NaiveDateTime>,
) -> Result<Delivery, AppError> {
    Deliveries::new(&mut *db, clock).create_for_order(
        order_id,
        NewDeliveryInput {
            contact_phone: "11912345678".to_owned(),
            fee: Money::from_cents(800),
            notes: "Portão azul".to_owned(),
            scheduled_for,
        },
    )
}

#[test]
fn delivery_requires_delivery_mode() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "P1", false)?;

    let err = create_delivery(&mut db, &clock, order_id, None).unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DeliveryNotRequired));
    Ok(())
}

#[test]
fn full_lifecycle_keeps_status_and_schedule_history() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "L1", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;

    let delivery = create_delivery(&mut db, &clock, order_id, None)?;
    assert_eq!(delivery.status(), DeliveryStatus::AwaitingScheduling);

    Deliveries::new(&mut db, &clock).schedule(delivery.id(), at(17, 9))?;
    Deliveries::new(&mut db, &clock).assign(delivery.id(), deliverer)?;
    clock.set(at(17, 8));
    Deliveries::new(&mut db, &clock).depart(delivery.id())?;
    clock.set(at(17, 11));
    let done = Deliveries::new(&mut db, &clock).complete(delivery.id(), "Ana Paula", "entregue no portão")?;

    assert_eq!(done.status(), DeliveryStatus::Delivered);
    assert_eq!(done.received_by(), Some("Ana Paula"));

    let history = Deliveries::new(&mut db, &clock).history(delivery.id())?;
    let statuses: Vec<DeliveryStatus> = history.status_changes.iter().map(|c| c.to).collect();
    assert_eq!(
        statuses,
        vec![
            DeliveryStatus::AwaitingScheduling,
            DeliveryStatus::Scheduled,
            DeliveryStatus::OutForDelivery,
            DeliveryStatus::Delivered,
        ]
    );
    assert_eq!(history.schedules.len(), 1);
    assert_eq!(history.assignments.len(), 1);
    Ok(())
}

#[test]
fn departure_requires_an_assigned_deliverer() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "L2", true)?;
    let delivery = create_delivery(&mut db, &clock, order_id, Some(at(17, 9)))?;

    let err = Deliveries::new(&mut db, &clock).depart(delivery.id()).unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DeliveryNotAssigned));
    Ok(())
}

#[test]
fn failed_delivery_can_be_rescheduled_keeping_previous_dates() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "R1", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    let delivery = create_delivery(&mut db, &clock, order_id, Some(at(17, 9)))?;

    Deliveries::new(&mut db, &clock).assign(delivery.id(), deliverer)?;
    clock.set(at(17, 9));
    Deliveries::new(&mut db, &clock).depart(delivery.id())?;
    clock.set(at(17, 11));
    let failed = Deliveries::new(&mut db, &clock).fail(delivery.id(), "cliente ausente")?;
    assert_eq!(failed.status(), DeliveryStatus::NotDelivered);

    let rescheduled = Deliveries::new(&mut db, &clock).schedule(delivery.id(), at(18, 9))?;
    assert_eq!(rescheduled.status(), DeliveryStatus::Scheduled);

    let history = Deliveries::new(&mut db, &clock).history(delivery.id())?;
    let dates: Vec<NaiveDateTime> = history.schedules.iter().map(|s| s.scheduled_for).collect();
    assert_eq!(dates, vec![at(17, 9), at(18, 9)]);
    assert_eq!(history.status_changes.len(), 5);
    Ok(())
}

#[test]
fn scheduling_in_the_past_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "P2", true)?;
    let delivery = create_delivery(&mut db, &clock, order_id, None)?;

    let err = Deliveries::new(&mut db, &clock)
        .schedule(delivery.id(), at(15, 9))
        .unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DeliveryDateInPast));
    Ok(())
}

#[test]
fn reassigning_closes_the_previous_assignment() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "A1", true)?;
    let first = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    let second = register_deliverer(&mut db, &clock, "Ana", CPF_ANA)?;
    let delivery = create_delivery(&mut db, &clock, order_id, None)?;

    Deliveries::new(&mut db, &clock).assign(delivery.id(), first)?;
    Deliveries::new(&mut db, &clock).assign(delivery.id(), second)?;

    let history = Deliveries::new(&mut db, &clock).history(delivery.id())?;
    assert_eq!(history.assignments.len(), 2);
    assert_eq!(history.assignments.iter().filter(|a| a.is_active()).count(), 1);
    assert_eq!(history.assignments[1].deliverer_id(), second);

    let repeated = Deliveries::new(&mut db, &clock)
        .assign(delivery.id(), second)
        .unwrap_err();
    assert_eq!(
        repeated,
        AppError::Validation(DomainError::AlreadyAssignedToDeliverer)
    );
    Ok(())
}

#[test]
fn inactive_deliverer_cannot_receive_deliveries() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "I1", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    Deliveries::new(&mut db, &clock).set_deliverer_active(deliverer, false)?;
    let delivery = create_delivery(&mut db, &clock, order_id, None)?;

    let err = Deliveries::new(&mut db, &clock)
        .assign(delivery.id(), deliverer)
        .unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DelivererInactive));

    // Mesmo acesso direto ao banco é bloqueado pelo trigger.
    let raw = db.connection().execute(
        "INSERT INTO delivery_assignments (id, delivery_id, deliverer_id, assigned_at, unassigned_at) \
         VALUES (?1, ?2, ?3, '2026-11-16T09:00:00', NULL)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            delivery.id().to_string(),
            deliverer.to_string()
        ],
    );
    assert!(raw.is_err());
    Ok(())
}

#[test]
fn cancelling_the_sale_cancels_the_open_delivery_and_releases_the_deliverer() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, sale_id) = setup_order(&mut db, &clock, "C1", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    let delivery = create_delivery(&mut db, &clock, order_id, Some(at(17, 9)))?;
    Deliveries::new(&mut db, &clock).assign(delivery.id(), deliverer)?;

    Sales::new(&mut db, &clock).cancel(sale_id, "cliente desistiu")?;

    let cancelled = Deliveries::new(&mut db, &clock).get(delivery.id())?;
    assert_eq!(cancelled.status(), DeliveryStatus::Cancelled);

    let history = Deliveries::new(&mut db, &clock).history(delivery.id())?;
    assert!(history.assignments.iter().all(|a| !a.is_active()));
    Ok(())
}

#[test]
fn sale_cannot_be_cancelled_while_out_for_delivery() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, sale_id) = setup_order(&mut db, &clock, "C2", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    let delivery = create_delivery(&mut db, &clock, order_id, Some(at(17, 9)))?;
    Deliveries::new(&mut db, &clock).assign(delivery.id(), deliverer)?;
    clock.set(at(17, 9));
    Deliveries::new(&mut db, &clock).depart(delivery.id())?;

    let err = Sales::new(&mut db, &clock)
        .cancel(sale_id, "cliente desistiu")
        .unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DeliveryInProgress));

    assert_eq!(Sales::new(&mut db, &clock).get(sale_id)?.status(), SaleStatus::Confirmed);
    Ok(())
}

#[test]
fn completing_a_delivery_does_not_mark_the_sale_as_paid() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, sale_id) = setup_order(&mut db, &clock, "D1", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    let delivery = create_delivery(&mut db, &clock, order_id, Some(at(17, 9)))?;
    Deliveries::new(&mut db, &clock).assign(delivery.id(), deliverer)?;
    clock.set(at(17, 9));
    Deliveries::new(&mut db, &clock).depart(delivery.id())?;
    Deliveries::new(&mut db, &clock).complete(delivery.id(), "Ana", "")?;

    let summary = Sales::new(&mut db, &clock).summary(sale_id)?;
    assert_eq!(summary.payment_status, PaymentStatus::Pending);
    Ok(())
}

#[test]
fn agenda_and_panel_reflect_scheduled_deliveries() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (first_order, _) = setup_order(&mut db, &clock, "G1", true)?;
    let (second_order, _) = setup_order(&mut db, &clock, "G2", true)?;
    create_delivery(&mut db, &clock, first_order, Some(at(17, 9)))?;
    create_delivery(&mut db, &clock, second_order, Some(at(18, 9)))?;

    let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("data de teste válida");
    let agenda = Deliveries::new(&mut db, &clock).daily_agenda(day)?;
    assert_eq!(agenda.len(), 1);

    let panel = Deliveries::new(&mut db, &clock).panel()?;
    assert_eq!(panel.awaiting_deliverer, 2);
    assert_eq!(panel.today, 0);
    assert_eq!(panel.overdue, 0);
    Ok(())
}

#[test]
fn only_one_delivery_per_order() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "U1", true)?;
    create_delivery(&mut db, &clock, order_id, None)?;

    let err = create_delivery(&mut db, &clock, order_id, None).unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DeliveryAlreadyExists));
    Ok(())
}

#[test]
fn deliverer_document_must_be_cpf() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let err = Deliveries::new(&mut db, &clock)
        .register_deliverer(DelivererData {
            name: "Empresa".to_owned(),
            phone: "85988887777".to_owned(),
            document: Some("11.222.333/0001-81".to_owned()),
            notes: String::new(),
        })
        .unwrap_err();
    assert_eq!(err, AppError::Validation(DomainError::DelivererDocumentMustBeCpf));
    Ok(())
}

#[test]
fn status_and_schedule_history_are_immutable() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "H1", true)?;
    create_delivery(&mut db, &clock, order_id, Some(at(17, 9)))?;

    let conn = db.connection();
    assert!(conn
        .execute("UPDATE delivery_status_history SET note = 'alterado'", [])
        .is_err());
    assert!(conn.execute("DELETE FROM delivery_status_history", []).is_err());
    assert!(conn.execute("UPDATE delivery_schedules SET scheduled_for = scheduled_for", []).is_err());
    assert!(conn.execute("DELETE FROM delivery_schedules", []).is_err());
    Ok(())
}

#[test]
fn search_filters_by_deliverer_through_active_assignment() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = MovableClock::new(at(16, 9));
    let (order_id, _) = setup_order(&mut db, &clock, "S1", true)?;
    let (other_order, _) = setup_order(&mut db, &clock, "S2", true)?;
    let deliverer = register_deliverer(&mut db, &clock, "Carlos", CPF_CARLOS)?;
    let assigned = create_delivery(&mut db, &clock, order_id, None)?;
    create_delivery(&mut db, &clock, other_order, None)?;
    Deliveries::new(&mut db, &clock).assign(assigned.id(), deliverer)?;

    let found = Deliveries::new(&mut db, &clock).search(&DeliveryFilter {
        deliverer_id: Some(deliverer),
        ..DeliveryFilter::default()
    })?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id(), assigned.id());
    Ok(())
}