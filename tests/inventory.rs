use std::error::Error;

use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use gestao_comercial::application::catalog::Catalog;
use gestao_comercial::application::clock::Clock;
use gestao_comercial::application::error::AppError;
use gestao_comercial::application::stock::StockControl;
use gestao_comercial::application::unit_of_work::Transactor;
use gestao_comercial::domain::category::Category;
use gestao_comercial::domain::errors::{DomainError, RepositoryError};
use gestao_comercial::domain::product::ProductData;
use gestao_comercial::domain::repositories::ProductFilter;
use gestao_comercial::domain::stock::{plan, MovementKind, StockMovement, StockOperation};
use gestao_comercial::domain::value_objects::Money;
use gestao_comercial::infrastructure::database::Database;

type TestResult = Result<(), Box<dyn Error>>;

struct FixedClock(NaiveDateTime);

impl Clock for FixedClock {
    fn now(&self) -> NaiveDateTime {
        self.0
    }
}

fn fixed_clock() -> FixedClock {
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).expect("data de teste válida");
    FixedClock(date.and_hms_opt(9, 0, 0).expect("horário de teste válido"))
}

fn product_data(sku: &str, min_stock: i64) -> ProductData {
    ProductData {
        sku: sku.to_owned(),
        name: "Cadeira plástica".to_owned(),
        description: String::new(),
        category_id: None,
        purchase_price: Money::from_cents(1_000),
        sale_price: Money::from_cents(2_500),
        min_stock,
        unit: "un".to_owned(),
    }
}

/// Cria um produto e, opcionalmente, dá entrada inicial de estoque.
fn seed_product(
    db: &mut Database,
    sku: &str,
    min_stock: i64,
    initial_stock: i64,
) -> Result<Uuid, Box<dyn Error>> {
    let clock = fixed_clock();
    let product = Catalog::new(&mut *db, &clock).create_product(product_data(sku, min_stock))?;
    if initial_stock > 0 {
        StockControl::new(&mut *db, &clock).register_entry(
            product.id(),
            initial_stock,
            "carga inicial",
        )?;
    }
    Ok(product.id())
}

#[test]
fn category_names_are_unique_ignoring_case() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = fixed_clock();

    Catalog::new(&mut db, &clock).create_category("Mesas")?;
    let err = Catalog::new(&mut db, &clock)
        .create_category("mesas")
        .unwrap_err();

    assert!(matches!(err, AppError::Conflict(_)));
    Ok(())
}

#[test]
fn sku_is_normalized_and_new_products_start_with_zero_stock() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = fixed_clock();

    let product = Catalog::new(&mut db, &clock).create_product(product_data("  cad-001 ", 2))?;

    assert_eq!(product.data().sku, "CAD-001");
    assert_eq!(product.stock(), 0);
    Ok(())
}

#[test]
fn duplicate_sku_is_reported_as_conflict() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = fixed_clock();

    Catalog::new(&mut db, &clock).create_product(product_data("ABC", 0))?;
    let err = Catalog::new(&mut db, &clock)
        .create_product(product_data("abc", 0))
        .unwrap_err();

    assert!(matches!(err, AppError::Conflict(_)));
    Ok(())
}

#[test]
fn product_requires_existing_category() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = fixed_clock();

    let data = ProductData {
        category_id: Some(Uuid::now_v7()),
        ..product_data("X1", 0)
    };
    let err = Catalog::new(&mut db, &clock).create_product(data).unwrap_err();

    assert!(matches!(err, AppError::NotFound("categoria")));
    Ok(())
}

#[test]
fn entry_increases_balance_and_records_movement() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "CAD-1", 0, 0)?;
    let clock = fixed_clock();

    let movement =
        StockControl::new(&mut db, &clock).register_entry(product_id, 10, "compra fornecedor")?;

    assert_eq!(movement.kind(), MovementKind::Entry);
    assert_eq!(movement.delta(), 10);
    assert_eq!(movement.balance_after(), 10);
    assert_eq!(movement.reason(), "compra fornecedor");

    let product = Catalog::new(&mut db, &clock).get_product(product_id)?;
    assert_eq!(product.stock(), 10);
    Ok(())
}

#[test]
fn exit_beyond_balance_is_rejected_without_side_effects() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "CAD-2", 0, 2)?;
    let clock = fixed_clock();

    let err = StockControl::new(&mut db, &clock)
        .register_exit(product_id, 5, "venda")
        .unwrap_err();

    assert!(matches!(
        err,
        AppError::Validation(DomainError::InsufficientStock { .. })
    ));
    let product = Catalog::new(&mut db, &clock).get_product(product_id)?;
    assert_eq!(product.stock(), 2);
    assert_eq!(
        StockControl::new(&mut db, &clock).movements_for(product_id)?.len(),
        1
    );
    Ok(())
}

#[test]
fn adjustment_records_the_difference() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "CAD-3", 0, 5)?;
    let clock = fixed_clock();

    let movement =
        StockControl::new(&mut db, &clock).adjust_stock(product_id, 8, "contagem mensal")?;
    assert_eq!(movement.kind(), MovementKind::Adjustment);
    assert_eq!(movement.delta(), 3);
    assert_eq!(movement.balance_after(), 8);

    let err = StockControl::new(&mut db, &clock)
        .adjust_stock(product_id, 8, "contagem mensal")
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::Validation(DomainError::NoStockChange)
    ));
    Ok(())
}

#[test]
fn movements_require_a_reason() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "CAD-4", 0, 0)?;
    let clock = fixed_clock();

    let err = StockControl::new(&mut db, &clock)
        .register_entry(product_id, 5, "   ")
        .unwrap_err();

    assert!(matches!(
        err,
        AppError::Validation(DomainError::EmptyField("motivo"))
    ));
    Ok(())
}

#[test]
fn inactive_product_cannot_move_stock() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "CAD-5", 0, 0)?;
    let clock = fixed_clock();

    Catalog::new(&mut db, &clock).set_product_active(product_id, false)?;
    let err = StockControl::new(&mut db, &clock)
        .register_entry(product_id, 1, "compra")
        .unwrap_err();

    assert!(matches!(
        err,
        AppError::Validation(DomainError::ProductInactive)
    ));
    Ok(())
}

#[test]
fn low_stock_includes_items_at_or_below_minimum() -> TestResult {
    let mut db = Database::open_in_memory()?;
    seed_product(&mut db, "A", 5, 3)?;  // abaixo do mínimo
    seed_product(&mut db, "B", 0, 10)?; // acima do mínimo
    seed_product(&mut db, "C", 2, 0)?;  // zerado, dentro do mínimo
    let clock = fixed_clock();

    let products = StockControl::new(&mut db, &clock).low_stock_products()?;
    let mut skus: Vec<String> = products.iter().map(|p| p.data().sku.clone()).collect();
    skus.sort();

    assert_eq!(skus, ["A", "C"]);
    Ok(())
}

#[test]
fn deactivated_products_are_hidden_from_default_search() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "OLD", 0, 0)?;
    let clock = fixed_clock();

    Catalog::new(&mut db, &clock).set_product_active(product_id, false)?;

    let visible = Catalog::new(&mut db, &clock).search_products(&ProductFilter::default())?;
    assert!(visible.is_empty());

    let filter = ProductFilter {
        include_inactive: true,
        ..ProductFilter::default()
    };
    let all = Catalog::new(&mut db, &clock).search_products(&filter)?;
    assert_eq!(all.len(), 1);
    Ok(())
}

#[test]
fn search_ignores_accents_and_case() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = fixed_clock();

    Catalog::new(&mut db, &clock).create_product(ProductData {
        name: "Cadeira Giratória".to_owned(),
        ..product_data("GIR-1", 0)
    })?;

    for text in ["giratoria", "CADEIRA", "  giratória "] {
        let filter = ProductFilter {
            text: Some(text.to_owned()),
            ..ProductFilter::default()
        };
        let found = Catalog::new(&mut db, &clock).search_products(&filter)?;
        assert_eq!(found.len(), 1, "busca por {text:?} deveria encontrar o produto");
    }
    Ok(())
}

#[test]
fn like_wildcards_are_matched_literally() -> TestResult {
    let mut db = Database::open_in_memory()?;
    seed_product(&mut db, "ABC", 0, 0)?;
    let clock = fixed_clock();

    for text in ["%", "_", "\\"] {
        let filter = ProductFilter {
            text: Some(text.to_owned()),
            ..ProductFilter::default()
        };
        let found = Catalog::new(&mut db, &clock).search_products(&filter)?;
        assert!(found.is_empty(), "{text:?} não deveria funcionar como curinga");
    }
    Ok(())
}

#[test]
fn updating_product_data_does_not_alter_stock() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "OLD", 0, 5)?;
    let clock = fixed_clock();

    let updated =
        Catalog::new(&mut db, &clock).update_product(product_id, product_data("NOVO", 4))?;
    assert_eq!(updated.data().sku, "NOVO");

    let stored = Catalog::new(&mut db, &clock).get_product(product_id)?;
    assert_eq!(stored.data().sku, "NOVO");
    assert_eq!(stored.data().min_stock, 4);
    assert_eq!(stored.stock(), 5);
    Ok(())
}

#[test]
fn stock_balance_equals_sum_of_movements() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "SUM", 0, 10)?;
    let clock = fixed_clock();

    StockControl::new(&mut db, &clock).register_exit(product_id, 4, "venda")?;
    StockControl::new(&mut db, &clock).adjust_stock(product_id, 3, "contagem")?;
    StockControl::new(&mut db, &clock).register_entry(product_id, 2, "compra")?;

    let movements = StockControl::new(&mut db, &clock).movements_for(product_id)?;
    let sum: i64 = movements.iter().map(|m| m.delta()).sum();
    let product = Catalog::new(&mut db, &clock).get_product(product_id)?;

    assert_eq!(product.stock(), 5);
    assert_eq!(sum, product.stock());
    Ok(())
}

#[test]
fn failed_unit_of_work_rolls_back_every_write() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let clock = fixed_clock();
    let category = Category::create(Uuid::now_v7(), "Temporária", clock.now())?;

    {
        let mut uow = db.begin()?;
        uow.insert_category(&category)?;
        // A unidade é descartada sem commit, como ocorreria após uma falha no meio da operação.
    }

    let categories = Catalog::new(&mut db, &clock).list_categories(false)?;
    assert!(categories.is_empty());
    Ok(())
}

#[test]
fn stale_previous_balance_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "STALE", 0, 5)?;
    let clock = fixed_clock();

    let planned = plan(5, StockOperation::Entry { quantity: 1 })?;
    let movement =
        StockMovement::new(Uuid::now_v7(), product_id, &planned, "teste", None, clock.now())?;

    let mut uow = db.begin()?;
    // Saldo informado (3) diferente do persistido (5): a gravação deve ser recusada.
    let result = uow.record_stock_movement(&movement, 3);
    assert!(matches!(result, Err(RepositoryError::Conflict(_))));
    drop(uow);

    let product = Catalog::new(&mut db, &clock).get_product(product_id)?;
    assert_eq!(product.stock(), 5);
    Ok(())
}

#[test]
fn product_with_movements_cannot_be_physically_deleted() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let product_id = seed_product(&mut db, "DEL", 0, 5)?;

    let result = db
        .connection()
        .execute("DELETE FROM products WHERE id = ?1", [product_id.to_string()]);

    assert!(result.is_err());
    Ok(())
}

#[test]
fn stock_movements_cannot_be_changed_or_deleted() -> TestResult {
    let mut db = Database::open_in_memory()?;
    seed_product(&mut db, "TRG", 0, 5)?;

    let update = db
        .connection()
        .execute("UPDATE stock_movements SET reason = 'alterado'", []);
    let delete = db.connection().execute("DELETE FROM stock_movements", []);

    assert!(update.is_err());
    assert!(delete.is_err());
    Ok(())
}