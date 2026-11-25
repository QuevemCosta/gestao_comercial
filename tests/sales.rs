use std::error::Error;

use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use gestao_comercial::application::catalog::Catalog;
use gestao_comercial::application::clock::Clock;
use gestao_comercial::application::customers::Customers;
use gestao_comercial::application::error::AppError;
use gestao_comercial::application::orders::{OrderItemInput, Orders};
use gestao_comercial::application::sales::Sales;
use gestao_comercial::application::stock::StockControl;
use gestao_comercial::domain::customer::{CustomerData, PersonType};
use gestao_comercial::domain::errors::DomainError;
use gestao_comercial::domain::order::{Order, OrderOrigin, OrderStatus, ReceiveMode};
use gestao_comercial::domain::payment::{PaymentMethod, PaymentStatus};
use gestao_comercial::domain::product::ProductData;
use gestao_comercial::domain::repositories::SaleFilter;
use gestao_comercial::domain::sale::SaleStatus;
use gestao_comercial::domain::stock::MovementKind;
use gestao_comercial::domain::value_objects::Money;
use gestao_comercial::infrastructure::database::Database;

type TestResult = Result<(), Box<dyn Error>>;

struct FixedClock(NaiveDateTime);

impl Clock for FixedClock {
    fn now(&self) -> NaiveDateTime {
        self.0
    }
}

fn clock_on(year: i32, month: u32, day: u32) -> FixedClock {
    let date = NaiveDate::from_ymd_opt(year, month, day).expect("data de teste válida");
    FixedClock(date.and_hms_opt(9, 0, 0).expect("horário de teste válido"))
}

fn new_customer(db: &mut Database, clock: &FixedClock) -> Result<Uuid, Box<dyn Error>> {
    let customer = Customers::new(&mut *db, clock).register(CustomerData {
        person_type: PersonType::Individual,
        name: "Ana Paula Lima".to_owned(),
        document: None,
        phone: Some("11912345678".to_owned()),
        whatsapp: None,
        address: None,
        notes: String::new(),
    })?;
    Ok(customer.id())
}

fn new_product(
    db: &mut Database,
    clock: &FixedClock,
    sku: &str,
    stock: i64,
) -> Result<Uuid, Box<dyn Error>> {
    let product = Catalog::new(&mut *db, clock).create_product(ProductData {
        sku: sku.to_owned(),
        name: "Cadeira plástica".to_owned(),
        description: String::new(),
        category_id: None,
        purchase_price: Money::from_cents(1_000),
        sale_price: Money::from_cents(2_500),
        min_stock: 0,
        unit: "un".to_owned(),
    })?;
    if stock > 0 {
        StockControl::new(&mut *db, clock).register_entry(product.id(), stock, "carga inicial")?;
    }
    Ok(product.id())
}

/// Cria um pedido em elaboração. Cada linha é (produto, quantidade, preço unitário em centavos).
fn draft_order(
    db: &mut Database,
    clock: &FixedClock,
    customer_id: Uuid,
    lines: &[(Uuid, i64, i64)],
) -> Result<Order, Box<dyn Error>> {
    let order = Orders::new(&mut *db, clock).create(
        customer_id,
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;
    let inputs: Vec<OrderItemInput> = lines
        .iter()
        .map(|(product_id, quantity, unit)| OrderItemInput {
            product_id: *product_id,
            quantity: *quantity,
            unit_price: Money::from_cents(*unit),
            discount: Money::ZERO,
        })
        .collect();
    Ok(Orders::new(&mut *db, clock).set_items(order.id(), inputs)?)
}

fn stock_of(db: &mut Database, clock: &FixedClock, product_id: Uuid) -> Result<i64, Box<dyn Error>> {
    Ok(Catalog::new(&mut *db, clock).get_product(product_id)?.stock())
}

#[test]
fn confirming_order_creates_sale_and_consumes_stock() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-1", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 3, 1000)])?;

    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;

    assert_eq!(sale.number(), "V-2026-0001");
    assert_eq!(sale.status(), SaleStatus::Confirmed);
    assert_eq!(sale.total()?, Money::from_cents(3000));
    assert_eq!(stock_of(&mut db, &clock, cadeira)?, 7);
    assert_eq!(Orders::new(&mut db, &clock).get(order.id())?.status(), OrderStatus::Confirmed);
    Ok(())
}

#[test]
fn order_discount_is_applied_to_sale_total() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-2", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 2, 1000)])?;
    Orders::new(&mut db, &clock).set_order_discount(order.id(), Money::from_cents(500))?;

    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;
    assert_eq!(sale.total()?, Money::from_cents(1500));
    Ok(())
}

#[test]
fn insufficient_stock_blocks_confirmation_without_side_effects() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-3", 2)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 5, 1000)])?;

    let err = Sales::new(&mut db, &clock).confirm_order(order.id()).unwrap_err();

    assert!(matches!(
        err,
        AppError::Validation(DomainError::InsufficientStock { .. })
    ));
    assert_eq!(Orders::new(&mut db, &clock).get(order.id())?.status(), OrderStatus::Draft);
    assert_eq!(stock_of(&mut db, &clock, cadeira)?, 2);
    assert!(Sales::new(&mut db, &clock).list(&SaleFilter::default())?.is_empty());
    Ok(())
}

#[test]
fn confirmed_order_cannot_be_confirmed_again() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-4", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    Sales::new(&mut db, &clock).confirm_order(order.id())?;

    let err = Sales::new(&mut db, &clock).confirm_order(order.id()).unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::OrderNotDraft)));
    assert_eq!(stock_of(&mut db, &clock, cadeira)?, 9);
    Ok(())
}

#[test]
fn sale_numbers_are_sequential_within_the_year() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-5", 10)?;

    let first = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let second = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;

    let sale_one = Sales::new(&mut db, &clock).confirm_order(first.id())?;
    let sale_two = Sales::new(&mut db, &clock).confirm_order(second.id())?;

    assert_eq!(sale_one.number(), "V-2026-0001");
    assert_eq!(sale_two.number(), "V-2026-0002");
    Ok(())
}

#[test]
fn cancelling_sale_reverses_stock_and_keeps_history() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-6", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 4, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;

    let cancellation = Sales::new(&mut db, &clock).cancel(sale.id(), "cliente desistiu")?;

    assert_eq!(cancellation.sale.status(), SaleStatus::Cancelled);
    assert_eq!(cancellation.sale.cancelled_reason(), Some("cliente desistiu"));
    assert_eq!(cancellation.received, Money::ZERO);
    assert_eq!(stock_of(&mut db, &clock, cadeira)?, 10);

    let movements = StockControl::new(&mut db, &clock).movements_for(cadeira)?;
    assert_eq!(movements.len(), 3, "entrada inicial, saída e estorno devem permanecer");
    let reversal = movements
        .iter()
        .find(|movement| movement.kind() == MovementKind::Reversal)
        .ok_or("estorno não encontrado")?;
    assert_eq!(reversal.delta(), 4);
    assert_eq!(movements.iter().map(|m| m.delta()).sum::<i64>(), 10);
    Ok(())
}

#[test]
fn cancelled_or_completed_sales_cannot_be_cancelled_again() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-7", 10)?;

    let first = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(first.id())?;
    Sales::new(&mut db, &clock).cancel(sale.id(), "erro")?;
    let err = Sales::new(&mut db, &clock).cancel(sale.id(), "outra vez").unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::SaleNotConfirmed)));

    let second = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let completed = Sales::new(&mut db, &clock).confirm_order(second.id())?;
    Sales::new(&mut db, &clock).complete(completed.id())?;
    let err = Sales::new(&mut db, &clock).cancel(completed.id(), "tarde demais").unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::SaleNotConfirmed)));
    Ok(())
}

#[test]
fn payments_move_status_from_pending_to_partial_to_paid() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-8", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 3, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;

    let initial = Sales::new(&mut db, &clock).summary(sale.id())?;
    assert_eq!(initial.payment_status, PaymentStatus::Pending);

    Sales::new(&mut db, &clock).register_payment(
        sale.id(),
        PaymentMethod::Cash,
        Money::from_cents(1000),
        "sinal",
    )?;
    let partial = Sales::new(&mut db, &clock).summary(sale.id())?;
    assert_eq!(partial.payment_status, PaymentStatus::Partial);
    assert_eq!(partial.balance, Money::from_cents(2000));

    Sales::new(&mut db, &clock).register_payment(
        sale.id(),
        PaymentMethod::Pix,
        Money::from_cents(2000),
        "",
    )?;
    let paid = Sales::new(&mut db, &clock).summary(sale.id())?;
    assert_eq!(paid.payment_status, PaymentStatus::Paid);
    assert_eq!(paid.balance, Money::ZERO);
    assert_eq!(paid.payments.len(), 2);
    Ok(())
}

#[test]
fn completing_a_sale_does_not_mark_it_as_paid() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-9", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;

    Sales::new(&mut db, &clock).complete(sale.id())?;

    let summary = Sales::new(&mut db, &clock).summary(sale.id())?;
    assert_eq!(summary.sale.status(), SaleStatus::Completed);
    assert_eq!(summary.payment_status, PaymentStatus::Pending);
    Ok(())
}

#[test]
fn overpayment_and_non_positive_amounts_are_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-10", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;

    let over = Sales::new(&mut db, &clock)
        .register_payment(sale.id(), PaymentMethod::Cash, Money::from_cents(1001), "")
        .unwrap_err();
    assert!(matches!(
        over,
        AppError::Validation(DomainError::PaymentExceedsBalance { .. })
    ));

    let zero = Sales::new(&mut db, &clock)
        .register_payment(sale.id(), PaymentMethod::Cash, Money::ZERO, "")
        .unwrap_err();
    assert!(matches!(
        zero,
        AppError::Validation(DomainError::NonPositivePayment)
    ));
    Ok(())
}

#[test]
fn cancelling_with_received_money_reports_the_amount() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-11", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 2, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;
    Sales::new(&mut db, &clock).register_payment(
        sale.id(),
        PaymentMethod::Pix,
        Money::from_cents(800),
        "adiantamento",
    )?;

    let cancellation = Sales::new(&mut db, &clock).cancel(sale.id(), "cliente desistiu")?;

    assert_eq!(cancellation.received, Money::from_cents(800));
    assert_eq!(cancellation.sale.status(), SaleStatus::Cancelled);
    Ok(())
}

#[test]
fn cancelled_sale_rejects_new_payments() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-12", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;
    Sales::new(&mut db, &clock).cancel(sale.id(), "erro")?;

    let err = Sales::new(&mut db, &clock)
        .register_payment(sale.id(), PaymentMethod::Cash, Money::from_cents(100), "")
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::SaleCancelled)));
    Ok(())
}

#[test]
fn payments_and_sale_items_are_immutable_in_the_database() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock_on(2026, 11, 16);
    let ana = new_customer(&mut db, &clock)?;
    let cadeira = new_product(&mut db, &clock, "CAD-13", 10)?;
    let order = draft_order(&mut db, &clock, ana, &[(cadeira, 1, 1000)])?;
    let sale = Sales::new(&mut db, &clock).confirm_order(order.id())?;
    Sales::new(&mut db, &clock).register_payment(sale.id(), PaymentMethod::Cash, Money::from_cents(500), "")?;

    let conn = db.connection();
    assert!(conn.execute("UPDATE payments SET amount_cents = 1", []).is_err());
    assert!(conn.execute("DELETE FROM payments", []).is_err());
    assert!(conn.execute("UPDATE sale_items SET quantity = 99", []).is_err());
    assert!(conn.execute("DELETE FROM sale_items", []).is_err());
    assert!(conn.execute("DELETE FROM sales", []).is_err());
    Ok(())
}

#[test]
fn sales_can_be_searched_by_number_and_period() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let november = clock_on(2026, 11, 16);
    let december = clock_on(2026, 12, 1);
    let ana = new_customer(&mut db, &november)?;
    let cadeira = new_product(&mut db, &november, "CAD-14", 10)?;

    let first_order = draft_order(&mut db, &november, ana, &[(cadeira, 1, 1000)])?;
    Sales::new(&mut db, &november).confirm_order(first_order.id())?;

    let second_order = draft_order(&mut db, &december, ana, &[(cadeira, 1, 1000)])?;
    let second = Sales::new(&mut db, &december).confirm_order(second_order.id())?;

    let by_number = Sales::new(&mut db, &november).list(&SaleFilter {
        number: Some("V-2026-0002".to_owned()),
        ..SaleFilter::default()
    })?;
    assert_eq!(by_number.len(), 1);
    assert_eq!(by_number[0].id(), second.id());

    let december_only = Sales::new(&mut db, &november).list(&SaleFilter {
        from: Some(NaiveDate::from_ymd_opt(2026, 12, 1).expect("data válida")),
        to: Some(NaiveDate::from_ymd_opt(2026, 12, 31).expect("data válida")),
        ..SaleFilter::default()
    })?;
    assert_eq!(december_only.len(), 1);

    let november_only = Sales::new(&mut db, &november).list(&SaleFilter {
        from: Some(NaiveDate::from_ymd_opt(2026, 11, 1).expect("data válida")),
        to: Some(NaiveDate::from_ymd_opt(2026, 11, 30).expect("data válida")),
        ..SaleFilter::default()
    })?;
    assert_eq!(november_only.len(), 1);
    Ok(())
}