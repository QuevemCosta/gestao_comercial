use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{NaiveDate, NaiveDateTime};

use gestao_comercial::application::reports::ReportTable;
use gestao_comercial::infrastructure::backup;
use gestao_comercial::infrastructure::database::Database;
use gestao_comercial::infrastructure::migrations::CURRENT_VERSION;
use gestao_comercial::presentation::controller::reports::to_csv;
use gestao_comercial::presentation::controller::{
    Controller, CustomerForm, ItemForm, OrderForm, ProductForm, StockAction,
};

type TestResult = Result<(), Box<dyn Error>>;

/// Posições dos relatórios na lista da tela (mesma ordem de `ReportKind::ALL`).
const REVENUE: i32 = 0;
const SALES_BY_CUSTOMER: i32 = 2;
const TOP_PRODUCTS: i32 = 3;
const PAYMENTS_RECEIVED: i32 = 4;
const PENDING_BALANCES: i32 = 5;
const LATE_RENTALS: i32 = 7;
const DELIVERIES_DONE: i32 = 8;
const PRODUCT_POSITIONS: i32 = 10;
const STOCK_MOVEMENTS: i32 = 11;

fn controller() -> Result<Controller, Box<dyn Error>> {
    Ok(Controller::new(Database::open_in_memory()?))
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("gc_{name}_{}_{nanos}", std::process::id()))
}

fn at(day: u32, hour: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, day)
        .and_then(|date| date.and_hms_opt(hour, 0, 0))
        .expect("data de teste válida")
}

fn audit_count(db: &Database) -> Result<i64, Box<dyn Error>> {
    Ok(db
        .connection()
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get::<_, i64>(0))?)
}

fn insert_audit(db: &Database) -> Result<(), Box<dyn Error>> {
    db.connection().execute(
        "INSERT INTO audit_log (occurred_at, action, entity) \
         VALUES ('2026-01-01T00:00:00', 'teste', 'produto')",
        [],
    )?;
    Ok(())
}

/// Valor de uma linha de relatório identificada pelo rótulo da primeira coluna.
fn cell(table: &ReportTable, label: &str) -> Option<String> {
    table
        .rows
        .iter()
        .find(|row| row.first().map(String::as_str) == Some(label))
        .and_then(|row| row.get(1).cloned())
}

/// Cadastra cliente e produto (25,00), dá entrada de 5 unidades, confirma venda de 2.
/// Devolve o identificador da venda. Todas as datas são "hoje" (relógio do sistema).
fn seed_confirmed_sale(c: &mut Controller) -> Result<String, Box<dyn Error>> {
    c.save_customer(CustomerForm {
        name: "Ana Paula Lima".into(),
        phone: "(85) 99999-8888".into(),
        ..CustomerForm::default()
    })?;
    c.save_product(ProductForm {
        sku: "cad-01".into(),
        name: "Cadeira".into(),
        purchase: "10,00".into(),
        sale: "25,00".into(),
        min_stock: "0".into(),
        unit: "un".into(),
        ..ProductForm::default()
    })?;
    let product_id = c.product_rows("")?[0].id.to_string();
    c.stock_operation(&product_id, StockAction::Entry, "5", "carga inicial")?;

    let order_id = c.save_order(OrderForm {
        customer_index: 0,
        mode_index: 1,
        discount: "0,00".into(),
        ..OrderForm::default()
    })?;
    c.add_order_item(
        &order_id,
        ItemForm {
            product_index: 0,
            quantity: "2".into(),
            ..ItemForm::default()
        },
    )?;
    c.confirm_order(&order_id)?;

    Ok(c.sale_rows("")?[0].id.to_string())
}

// ---------- Relatórios ----------

#[test]
fn revenue_report_reflects_confirmed_sale_and_payments() -> TestResult {
    let mut c = controller()?;
    let sale_id = seed_confirmed_sale(&mut c)?;
    c.register_payment(&sale_id, 1, "20,00", "sinal")?;

    let table = c.report_table(REVENUE, "", "")?;

    assert_eq!(cell(&table, "Total vendido").as_deref(), Some("R$ 50,00"));
    assert_eq!(cell(&table, "Recebido no período").as_deref(), Some("R$ 20,00"));
    assert_eq!(cell(&table, "Saldo das vendas do período").as_deref(), Some("R$ 30,00"));
    assert_eq!(cell(&table, "Recebido em PIX").as_deref(), Some("R$ 20,00"));
    assert_eq!(cell(&table, "Recebido em Dinheiro").as_deref(), Some("R$ 0,00"));
    Ok(())
}

#[test]
fn pending_balances_drop_out_after_full_payment() -> TestResult {
    let mut c = controller()?;
    let sale_id = seed_confirmed_sale(&mut c)?;

    let before = c.report_table(PENDING_BALANCES, "", "")?;
    assert!(before.summary.starts_with("1 pendência"));

    // Método 0 = dinheiro; valor igual ao total da venda (2 x 25,00).
    c.register_payment(&sale_id, 0, "50,00", "")?;

    let after = c.report_table(PENDING_BALANCES, "", "")?;
    assert!(after.summary.starts_with("0 pendência"));
    assert!(after.rows.is_empty());
    Ok(())
}

#[test]
fn sales_by_customer_groups_totals_per_customer() -> TestResult {
    let mut c = controller()?;
    seed_confirmed_sale(&mut c)?;

    let table = c.report_table(SALES_BY_CUSTOMER, "", "")?;

    assert_eq!(table.rows.len(), 1);
    assert_eq!(
        table.rows[0],
        vec![
            "Ana Paula Lima".to_owned(),
            "1".to_owned(),
            "R$ 50,00".to_owned(),
        ]
    );
    Ok(())
}

#[test]
fn top_products_ranks_by_quantity_sold() -> TestResult {
    let mut c = controller()?;
    seed_confirmed_sale(&mut c)?;

    let table = c.report_table(TOP_PRODUCTS, "", "")?;

    assert_eq!(table.rows.len(), 1);
    assert!(table.rows[0][0].starts_with("CAD-01"));
    assert_eq!(table.rows[0][1], "2");
    assert_eq!(table.rows[0][2], "R$ 50,00");
    Ok(())
}

#[test]
fn payments_report_lists_received_money_with_origin() -> TestResult {
    let mut c = controller()?;
    let sale_id = seed_confirmed_sale(&mut c)?;
    c.register_payment(&sale_id, 1, "20,00", "sinal")?;

    let table = c.report_table(PAYMENTS_RECEIVED, "", "")?;

    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0][1], "PIX");
    assert_eq!(table.rows[0][2], "Venda");
    assert_eq!(table.rows[0][3], "R$ 20,00");
    assert_eq!(table.summary, "1 pagamento(s) · total R$ 20,00");
    Ok(())
}

#[test]
fn product_positions_report_shows_current_stock() -> TestResult {
    let mut c = controller()?;
    seed_confirmed_sale(&mut c)?;

    let table = c.report_table(PRODUCT_POSITIONS, "", "")?;

    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0][2], "3", "5 de entrada menos 2 vendidas");
    assert!(table.summary.contains("1 produto(s)"));
    Ok(())
}

#[test]
fn stock_movements_report_includes_exits_from_sales() -> TestResult {
    let mut c = controller()?;
    seed_confirmed_sale(&mut c)?;

    let table = c.report_table(STOCK_MOVEMENTS, "", "")?;

    assert_eq!(table.rows.len(), 2, "entrada inicial e saída da venda");
    assert!(table.rows.iter().any(|row| row[2] == "Saída" && row[3] == "-2"));
    Ok(())
}

#[test]
fn situational_reports_are_empty_on_a_fresh_database() -> TestResult {
    let mut c = controller()?;

    let late = c.report_table(LATE_RENTALS, "", "")?;
    assert!(late.summary.starts_with("0 locação"));

    let done = c.report_table(DELIVERIES_DONE, "", "")?;
    assert!(done.summary.starts_with("0 entrega"));
    Ok(())
}

#[test]
fn invalid_period_is_rejected_with_clear_message() -> TestResult {
    let mut c = controller()?;
    let err = c
        .report_table(REVENUE, "10/11/2026", "01/11/2026")
        .unwrap_err();
    assert!(err.contains("data final"));
    Ok(())
}

#[test]
fn malformed_date_is_rejected() -> TestResult {
    let mut c = controller()?;
    let err = c.report_table(REVENUE, "2026-11-01", "").unwrap_err();
    assert!(err.contains("dd/mm/aaaa"));
    Ok(())
}

#[test]
fn csv_export_has_bom_semicolons_and_escaped_cells() -> TestResult {
    let table = ReportTable {
        title: "Teste".to_owned(),
        headers: vec!["Coluna A".to_owned(), "Coluna B".to_owned()],
        rows: vec![vec!["x;y".to_owned(), "diz \"oi\"".to_owned()]],
        summary: String::new(),
    };

    let csv = to_csv(&table);

    assert!(csv.starts_with('\u{feff}'));
    assert!(csv.contains("Coluna A;Coluna B\n"));
    assert!(csv.contains("\"x;y\";\"diz \"\"oi\"\"\""));
    Ok(())
}

// ---------- Configurações e backup ----------

#[test]
fn settings_view_reports_current_schema_version() -> TestResult {
    let mut c = controller()?;
    let view = c.settings_view()?;
    assert_eq!(view.schema_version, CURRENT_VERSION.to_string());
    Ok(())
}

#[test]
fn restore_without_confirmation_has_no_effect() -> TestResult {
    let mut c = controller()?;
    let err = c.prepare_restore(0, false).unwrap_err();
    assert!(err.contains("confirmação"));
    Ok(())
}

#[test]
fn backup_round_trip_restores_the_previous_content() -> TestResult {
    let dir = temp_dir("restore");
    let db_path = dir.join("gestao.db");
    let now = at(16, 9);

    let backup_path = {
        let db = Database::open(&db_path)?;
        insert_audit(&db)?;
        let path = backup::create_backup(db.connection(), &dir.join("backups"), now)?;
        // Alteração posterior ao backup: deve ser substituída pela restauração.
        insert_audit(&db)?;
        assert_eq!(audit_count(&db)?, 2);
        path
    };

    backup::stage_restore(&backup_path, &db_path)?;
    assert!(backup::pending_restore_path(&db_path).exists());

    let kept = backup::apply_pending_restore(&db_path, now)?
        .ok_or("a restauração pendente deveria ter sido aplicada")?;
    assert!(kept.exists(), "o banco anterior deve ser preservado");
    assert!(!backup::pending_restore_path(&db_path).exists());

    let restored = Database::open(&db_path)?;
    assert_eq!(audit_count(&restored)?, 1);
    drop(restored);

    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn backup_name_contains_date_and_time_and_same_second_is_refused() -> TestResult {
    let dir = temp_dir("names");
    let db_path = dir.join("gestao.db");
    let db = Database::open(&db_path)?;
    let backups = dir.join("backups");
    let now = at(16, 9);

    let first = backup::create_backup(db.connection(), &backups, now)?;
    let name = first
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    assert_eq!(name, "backup-20261116-090000.db");

    assert!(backup::create_backup(db.connection(), &backups, now).is_err());

    drop(db);
    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn corrupted_backup_is_rejected_and_nothing_is_staged() -> TestResult {
    let dir = temp_dir("corrupt");
    fs::create_dir_all(&dir)?;
    let db_path = dir.join("gestao.db");
    let bad = dir.join("backup-ruim.db");
    fs::write(&bad, b"isto nao e um banco de dados")?;

    assert!(backup::stage_restore(&bad, &db_path).is_err());
    assert!(!backup::pending_restore_path(&db_path).exists());

    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn backups_are_listed_most_recent_first() -> TestResult {
    let dir = temp_dir("list");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("backup-20260101-000000.db"), b"a")?;
    fs::write(dir.join("backup-20260102-000000.db"), b"b")?;
    fs::write(dir.join("outro-arquivo.txt"), b"c")?;

    let listed = backup::list_backups(&dir)?;

    assert_eq!(listed.len(), 2);
    assert!(listed[0]
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .contains("20260102"));

    fs::remove_dir_all(&dir)?;
    Ok(())
}