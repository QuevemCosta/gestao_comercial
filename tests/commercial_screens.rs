use std::error::Error;

use gestao_comercial::infrastructure::database::Database;
use gestao_comercial::presentation::controller::{
    Controller, CustomerForm, ItemForm, OrderForm, ProductForm, StockAction,
};

type TestResult = Result<(), Box<dyn Error>>;

fn controller() -> Result<Controller, Box<dyn Error>> {
    Ok(Controller::new(Database::open_in_memory()?))
}

/// Cadastra um cliente e um produto (preço 25,00) com o estoque informado.
fn seed(c: &mut Controller, stock: &str) -> Result<(), Box<dyn Error>> {
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
    let rows = c.product_rows("")?;
    let product_id = rows[0].id.as_str().to_owned();
    c.stock_operation(&product_id, StockAction::Entry, stock, "carga inicial")?;
    c.load_lookups()?;
    Ok(())
}

fn pickup_order(c: &mut Controller) -> Result<String, Box<dyn Error>> {
    Ok(c.save_order(OrderForm {
        customer_index: 0,
        mode_index: 1,
        discount: "0,00".into(),
        ..OrderForm::default()
    })?)
}

fn add_two_chairs(c: &mut Controller, order_id: &str) -> Result<(), Box<dyn Error>> {
    c.add_order_item(
        order_id,
        ItemForm {
            product_index: 0,
            quantity: "2".into(),
            ..ItemForm::default()
        },
    )?;
    Ok(())
}

#[test]
fn order_is_built_confirmed_and_consumes_stock() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "5")?;

    let order_id = pickup_order(&mut c)?;
    add_two_chairs(&mut c, &order_id)?;

    let view = c.load_order(&order_id)?;
    assert!(view.editable);
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.items[0].total.as_str(), "R$ 50,00");

    let message = c.confirm_order(&order_id)?;
    assert!(message.contains("V-"));

    let rows = c.product_rows("")?;
    assert_eq!(rows[0].stock.as_str(), "3");
    assert!(!c.load_order(&order_id)?.editable);
    Ok(())
}

#[test]
fn discount_is_rejected_on_new_order() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "5")?;

    let err = c
        .save_order(OrderForm {
            customer_index: 0,
            mode_index: 1,
            discount: "1,00".into(),
            ..OrderForm::default()
        })
        .unwrap_err();
    assert!(err.contains("itens"));
    Ok(())
}

#[test]
fn removing_an_item_keeps_the_order_editable() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "5")?;

    let order_id = pickup_order(&mut c)?;
    add_two_chairs(&mut c, &order_id)?;
    c.remove_order_item(&order_id, 0)?;

    let view = c.load_order(&order_id)?;
    assert!(view.items.is_empty());
    assert!(view.editable);
    Ok(())
}

#[test]
fn partial_then_full_payment_clears_the_balance_list() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "5")?;

    let order_id = pickup_order(&mut c)?;
    add_two_chairs(&mut c, &order_id)?;
    c.confirm_order(&order_id)?;

    let sales = c.sale_rows("")?;
    let sale_id = sales[0].id.as_str().to_owned();
    assert_eq!(c.balance_rows()?.len(), 1);

    // Método 1 = PIX.
    c.register_payment(&sale_id, 1, "20,00", "sinal")?;
    let partial = c.load_payment_view(&sale_id)?;
    assert!(partial.summary.contains("Parcial"));

    // Método 0 = dinheiro.
    c.register_payment(&sale_id, 0, "30,00", "")?;
    assert!(c.balance_rows()?.is_empty());
    Ok(())
}

#[test]
fn cancelling_a_sale_with_received_money_asks_for_manual_handling() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "5")?;

    let order_id = pickup_order(&mut c)?;
    add_two_chairs(&mut c, &order_id)?;
    c.confirm_order(&order_id)?;

    let sales = c.sale_rows("")?;
    let sale_id = sales[0].id.as_str().to_owned();
    c.register_payment(&sale_id, 0, "10,00", "")?;

    let message = c.cancel_sale(&sale_id, "cliente desistiu")?;
    assert!(message.contains("tratamento manual"));

    let rows = c.product_rows("")?;
    assert_eq!(rows[0].stock.as_str(), "5");
    Ok(())
}