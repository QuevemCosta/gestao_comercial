use std::error::Error;

use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use gestao_comercial::application::catalog::Catalog;
use gestao_comercial::application::clock::Clock;
use gestao_comercial::application::customers::Customers;
use gestao_comercial::application::error::AppError;
use gestao_comercial::application::orders::{OrderItemInput, Orders};
use gestao_comercial::application::stock::StockControl;
use gestao_comercial::domain::customer::{Address, Customer, CustomerData, PersonType};
use gestao_comercial::domain::errors::DomainError;
use gestao_comercial::domain::order::{OrderOrigin, OrderStatus, ReceiveMode};
use gestao_comercial::domain::product::ProductData;
use gestao_comercial::domain::repositories::{CustomerFilter, OrderFilter};
use gestao_comercial::domain::value_objects::Money;
use gestao_comercial::infrastructure::database::Database;

type TestResult = Result<(), Box<dyn Error>>;

struct FixedClock(NaiveDateTime);

impl Clock for FixedClock {
    fn now(&self) -> NaiveDateTime {
        self.0
    }
}

fn clock() -> FixedClock {
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).expect("data de teste válida");
    FixedClock(date.and_hms_opt(9, 0, 0).expect("horário de teste válido"))
}

fn customer_data(name: &str, phone: &str) -> CustomerData {
    CustomerData {
        person_type: PersonType::Individual,
        name: name.to_owned(),
        document: None,
        phone: Some(phone.to_owned()),
        whatsapp: None,
        address: None,
        notes: String::new(),
    }
}

fn address() -> Result<Address, DomainError> {
    Address::new("Rua das Flores, 10", "Centro", "Fortaleza")
}

fn register(db: &mut Database, data: CustomerData) -> Result<Customer, Box<dyn Error>> {
    let clock = clock();
    Ok(Customers::new(&mut *db, &clock).register(data)?)
}

fn new_product(db: &mut Database, sku: &str, stock: i64) -> Result<Uuid, Box<dyn Error>> {
    let clock = clock();
    let product = Catalog::new(&mut *db, &clock).create_product(ProductData {
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
        StockControl::new(&mut *db, &clock).register_entry(product.id(), stock, "carga inicial")?;
    }
    Ok(product.id())
}

fn input(product_id: Uuid, quantity: i64, unit: i64, discount: i64) -> OrderItemInput {
    OrderItemInput {
        product_id,
        quantity,
        unit_price: Money::from_cents(unit),
        discount: Money::from_cents(discount),
    }
}

#[test]
fn customer_without_contact_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let data = CustomerData {
        phone: None,
        ..customer_data("Ana", "11912345678")
    };
    let err = Customers::new(&mut db, &clock).register(data).unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::ContactRequired)));
    Ok(())
}

#[test]
fn cpf_for_company_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let data = CustomerData {
        person_type: PersonType::Company,
        document: Some("529.982.247-25".to_owned()),
        ..customer_data("Loja X", "11912345678")
    };
    let err = Customers::new(&mut db, &clock).register(data).unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::TaxIdKindMismatch)));
    Ok(())
}

#[test]
fn invalid_document_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let data = CustomerData {
        document: Some("12345678901".to_owned()),
        ..customer_data("Ana", "11912345678")
    };
    let err = Customers::new(&mut db, &clock).register(data).unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::InvalidTaxId)));
    Ok(())
}

#[test]
fn duplicate_document_is_reported_as_conflict() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let first = CustomerData {
        document: Some("529.982.247-25".to_owned()),
        ..customer_data("Ana Paula Lima", "11912345678")
    };
    let second = CustomerData {
        document: Some("52998224725".to_owned()),
        ..customer_data("Outra Pessoa", "11987654321")
    };
    Customers::new(&mut db, &clock).register(first)?;
    let err = Customers::new(&mut db, &clock).register(second).unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)));
    Ok(())
}

#[test]
fn customer_search_matches_name_document_and_phone() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    register(
        &mut db,
        CustomerData {
            document: Some("529.982.247-25".to_owned()),
            ..customer_data("Ana Paula Lima", "(11) 91234-5678")
        },
    )?;
    register(&mut db, customer_data("Bruno Gonçalves", "+55 85 3456-7890"))?;

    let mut search = |text: &str| -> Result<Vec<Customer>, Box<dyn Error>> {
        let filter = CustomerFilter {
            text: Some(text.to_owned()),
            ..CustomerFilter::default()
        };
        Ok(Customers::new(&mut db, &clock).search(&filter)?)
    };

    let by_name = search("ana")?;
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].data().name, "Ana Paula Lima");

    let by_accented_name = search("goncalves")?;
    assert_eq!(by_accented_name.len(), 1);

    let by_document = search("529.982.247-25")?;
    assert_eq!(by_document.len(), 1);

    let by_phone = search("(11) 91234-5678")?;
    assert_eq!(by_phone.len(), 1);

    let by_partial_phone = search("3456-7890")?;
    assert_eq!(by_partial_phone.len(), 1);
    Ok(())
}

#[test]
fn inactive_customer_cannot_place_order() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    Customers::new(&mut db, &clock).set_active(ana.id(), false)?;

    let err = Orders::new(&mut db, &clock)
        .create(ana.id(), OrderOrigin::Whatsapp, ReceiveMode::Pickup, None)
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::CustomerInactive)));
    Ok(())
}

#[test]
fn delivery_order_keeps_address_snapshot_after_customer_changes() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(
        &mut db,
        CustomerData {
            address: Some(address()?),
            ..customer_data("Ana Paula Lima", "11912345678")
        },
    )?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Delivery,
        None,
    )?;

    Customers::new(&mut db, &clock).update(
        ana.id(),
        CustomerData {
            address: Some(Address::new("Av. Nova, 99", "Meireles", "Fortaleza")?),
            ..customer_data("Ana Paula Lima", "11912345678")
        },
    )?;

    let stored = Orders::new(&mut db, &clock).get(order.id())?;
    let delivery = stored.delivery_address().ok_or("pedido sem endereço")?;
    assert_eq!(delivery.street(), "Rua das Flores, 10");
    Ok(())
}

#[test]
fn delivery_without_any_address_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let err = Orders::new(&mut db, &clock)
        .create(ana.id(), OrderOrigin::Whatsapp, ReceiveMode::Delivery, None)
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::DeliveryAddressRequired)));
    Ok(())
}

#[test]
fn pickup_order_has_no_delivery_address() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::InPerson,
        ReceiveMode::Pickup,
        Some(address()?),
    )?;
    assert!(order.delivery_address().is_none());
    Ok(())
}

#[test]
fn order_totals_apply_item_and_order_discounts() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-1", 0)?;
    let mesa = new_product(&mut db, "MES-1", 0)?;

    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;
    let order = Orders::new(&mut db, &clock).set_items(
        order.id(),
        vec![input(cadeira, 3, 1000, 500), input(mesa, 2, 2500, 0)],
    )?;
    let order = Orders::new(&mut db, &clock).set_order_discount(order.id(), Money::from_cents(1000))?;

    assert_eq!(order.subtotal()?, Money::from_cents(8000));
    assert_eq!(order.items_discount()?, Money::from_cents(500));
    assert_eq!(order.total()?, Money::from_cents(6500));
    Ok(())
}

#[test]
fn item_discount_above_gross_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-2", 0)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;

    let err = Orders::new(&mut db, &clock)
        .set_items(order.id(), vec![input(cadeira, 1, 1000, 1500)])
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::DiscountExceedsAmount)));
    Ok(())
}

#[test]
fn order_discount_above_items_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-3", 0)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;
    let order = Orders::new(&mut db, &clock).set_items(order.id(), vec![input(cadeira, 1, 1000, 0)])?;

    let err = Orders::new(&mut db, &clock)
        .set_order_discount(order.id(), Money::from_cents(5000))
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::DiscountExceedsAmount)));
    Ok(())
}

#[test]
fn duplicate_product_in_order_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-4", 0)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;

    let err = Orders::new(&mut db, &clock)
        .set_items(
            order.id(),
            vec![input(cadeira, 1, 1000, 0), input(cadeira, 2, 1000, 0)],
        )
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::DuplicateProduct)));
    Ok(())
}

#[test]
fn inactive_product_cannot_be_added_to_order() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-5", 0)?;
    Catalog::new(&mut db, &clock).set_product_active(cadeira, false)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;

    let err = Orders::new(&mut db, &clock)
        .set_items(order.id(), vec![input(cadeira, 1, 1000, 0)])
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::ProductInactive)));
    Ok(())
}

#[test]
fn adding_items_to_order_does_not_touch_stock() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let mesa = new_product(&mut db, "MES-2", 5)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;

    Orders::new(&mut db, &clock).set_items(order.id(), vec![input(mesa, 2, 2500, 0)])?;

    let product = Catalog::new(&mut db, &clock).get_product(mesa)?;
    assert_eq!(product.stock(), 5);
    assert_eq!(StockControl::new(&mut db, &clock).movements_for(mesa)?.len(), 1);
    Ok(())
}

#[test]
fn items_keep_the_order_they_were_entered() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-6", 0)?;
    let mesa = new_product(&mut db, "MES-3", 0)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;

    let order = Orders::new(&mut db, &clock).set_items(
        order.id(),
        vec![input(mesa, 1, 2500, 0), input(cadeira, 1, 1000, 0)],
    )?;
    let stored = Orders::new(&mut db, &clock).get(order.id())?;

    assert_eq!(stored.items()[0].product_id(), mesa);
    assert_eq!(stored.items()[1].product_id(), cadeira);
    Ok(())
}

#[test]
fn cancelled_order_is_frozen_in_domain_and_database() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let cadeira = new_product(&mut db, "CAD-7", 0)?;
    let order = Orders::new(&mut db, &clock).create(
        ana.id(),
        OrderOrigin::Whatsapp,
        ReceiveMode::Pickup,
        None,
    )?;
    Orders::new(&mut db, &clock).set_items(order.id(), vec![input(cadeira, 1, 1000, 0)])?;

    let cancelled = Orders::new(&mut db, &clock).cancel(order.id())?;
    assert_eq!(cancelled.status(), OrderStatus::Cancelled);

    let err = Orders::new(&mut db, &clock)
        .set_items(order.id(), Vec::new())
        .unwrap_err();
    assert!(matches!(err, AppError::Validation(DomainError::OrderNotDraft)));

    // Mesmo acesso direto ao banco deve ser bloqueado pelo trigger.
    let raw = db.connection().execute("UPDATE order_items SET quantity = 99", []);
    assert!(raw.is_err());
    Ok(())
}

#[test]
fn orders_can_be_listed_by_customer() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = clock();
    let ana = register(&mut db, customer_data("Ana Paula Lima", "11912345678"))?;
    let bruno = register(&mut db, customer_data("Bruno Gonçalves", "11987654321"))?;

    Orders::new(&mut db, &clock).create(ana.id(), OrderOrigin::Whatsapp, ReceiveMode::Pickup, None)?;
    Orders::new(&mut db, &clock).create(ana.id(), OrderOrigin::InPerson, ReceiveMode::Pickup, None)?;
    Orders::new(&mut db, &clock).create(bruno.id(), OrderOrigin::Other, ReceiveMode::Pickup, None)?;

    let filter = OrderFilter {
        customer_id: Some(ana.id()),
        ..OrderFilter::default()
    };
    let orders = Orders::new(&mut db, &clock).list(&filter)?;
    assert_eq!(orders.len(), 2);
    Ok(())
}