use std::error::Error;

use gestao_comercial::infrastructure::database::Database;
use gestao_comercial::presentation::controller::{
    Controller, CustomerForm, ProductForm, StockAction,
};

type TestResult = Result<(), Box<dyn Error>>;

fn controller() -> Result<Controller, Box<dyn Error>> {
    Ok(Controller::new(Database::open_in_memory()?))
}

fn product_form(sku: &str, sale: &str) -> ProductForm {
    ProductForm {
        sku: sku.to_owned(),
        name: "Cadeira plástica".to_owned(),
        purchase: "10,00".to_owned(),
        sale: sale.to_owned(),
        min_stock: "2".to_owned(),
        unit: "un".to_owned(),
        ..ProductForm::default()
    }
}

#[test]
fn saved_product_appears_formatted_in_the_list() -> TestResult {
    let mut c = controller()?;
    c.save_product(product_form("cad-01", "25,50"))?;

    let rows = c.product_rows("")?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sku.as_str(), "CAD-01");
    assert_eq!(rows[0].sale_price.as_str(), "R$ 25,50");
    assert_eq!(rows[0].status.as_str(), "Sem estoque");
    Ok(())
}

#[test]
fn invalid_price_is_reported_and_nothing_is_saved() -> TestResult {
    let mut c = controller()?;
    let err = c.save_product(product_form("cad-02", "abc")).unwrap_err();
    assert!(err.contains("preço de venda"));
    assert!(c.product_rows("")?.is_empty());
    Ok(())
}

#[test]
fn product_can_be_linked_to_a_category() -> TestResult {
    let mut c = controller()?;
    c.save_category("", "Mobiliário")?;
    c.load_categories()?;
    assert_eq!(c.category_options().len(), 2);

    let mut form = product_form("mes-01", "100,00");
    form.category_index = 1;
    c.save_product(form)?;

    let rows = c.product_rows("")?;
    assert_eq!(rows[0].category.as_str(), "Mobiliário");
    Ok(())
}

#[test]
fn stock_movements_follow_operations_and_block_overdraw() -> TestResult {
    let mut c = controller()?;
    c.save_product(product_form("cad-03", "25,00"))?;
    let id = c.product_rows("")?[0].id.to_string();

    c.stock_operation(&id, StockAction::Entry, "5", "compra")?;
    let (label, movements) = c.select_stock_item(&id)?;
    assert!(label.contains("CAD-03"));
    assert_eq!(movements.len(), 1);
    assert_eq!(movements[0].delta.as_str(), "+5");

    let err = c
        .stock_operation(&id, StockAction::Exit, "9", "venda")
        .unwrap_err();
    assert!(err.contains("estoque insuficiente"));
    Ok(())
}

#[test]
fn customer_form_validates_document_and_address() -> TestResult {
    let mut c = controller()?;
    let base = CustomerForm {
        name: "Ana Paula Lima".to_owned(),
        phone: "(85) 99999-8888".to_owned(),
        ..CustomerForm::default()
    };

    let bad_document = CustomerForm {
        document: "12345678901".to_owned(),
        ..base.clone()
    };
    assert!(c.save_customer(bad_document).unwrap_err().contains("CPF"));

    let partial_address = CustomerForm {
        street: "Rua A, 1".to_owned(),
        ..base.clone()
    };
    assert!(c
        .save_customer(partial_address)
        .unwrap_err()
        .contains("logradouro"));

    c.save_customer(CustomerForm {
        document: "529.982.247-25".to_owned(),
        ..base
    })?;
    assert_eq!(c.customer_rows("ana")?.len(), 1);
    Ok(())
}

#[test]
fn dashboard_exposes_all_indicator_cards() -> TestResult {
    let mut c = controller()?;
    let cards = c.dashboard()?;
    assert_eq!(cards.len(), 12);
    assert_eq!(cards[0].label.as_str(), "Vendas do dia");
    assert_eq!(cards[0].value.as_str(), "0");
    Ok(())
}