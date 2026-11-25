use std::error::Error;

use gestao_comercial::infrastructure::database::Database;
use gestao_comercial::presentation::controller::logistics::{
    DelivererForm, DraftLine, NewDeliveryForm, RentalForm, RentalItemForm,
};
use gestao_comercial::presentation::controller::{
    Controller, CustomerForm, ItemForm, OrderForm, ProductForm, StockAction,
};

type TestResult = Result<(), Box<dyn Error>>;

/// Datas no futuro distante: os testes não dependem da data em que rodam.
const PICKUP: &str = "10/01/2099 08:00";
const RETURN: &str = "10/01/2099 18:00";

fn controller() -> Result<Controller, Box<dyn Error>> {
    Ok(Controller::new(Database::open_in_memory()?))
}

/// Cadastra um cliente e um item de locação com a quantidade informada.
fn seed(c: &mut Controller, chairs: &str) -> TestResult {
    c.save_customer(CustomerForm {
        name: "Ana Paula Lima".into(),
        phone: "(85) 99999-8888".into(),
        ..CustomerForm::default()
    })?;
    c.save_rental_item(RentalItemForm {
        name: "Cadeira".into(),
        category: "Mobiliário".into(),
        price: "5,00".into(),
        initial_total: chairs.into(),
        ..RentalItemForm::default()
    })?;
    c.load_lookups()?;
    Ok(())
}

fn draft(c: &mut Controller, quantity: &str) -> Result<Vec<DraftLine>, Box<dyn Error>> {
    let row = c.rental_draft_line(0, quantity, "")?;
    Ok(vec![DraftLine {
        item_id: row.item_id.to_string(),
        quantity: row.quantity.to_string(),
        price: row.price.to_string(),
    }])
}

fn rental_form() -> RentalForm {
    RentalForm {
        customer_index: 0,
        pickup: PICKUP.into(),
        expected: RETURN.into(),
        discount: "0,00".into(),
    }
}

#[test]
fn rental_respects_period_capacity() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "10")?;
    let lines = draft(&mut c, "6")?;

    c.create_rental(rental_form(), &lines)?;
    let err = c.create_rental(rental_form(), &lines).unwrap_err();

    assert!(err.contains("disponibilidade insuficiente"));
    Ok(())
}

#[test]
fn rental_closes_when_every_unit_is_accounted_for() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "10")?;
    let lines = draft(&mut c, "3")?;
    c.create_rental(rental_form(), &lines)?;

    let rental_id = c.rental_rows()?[0].id.to_string();
    assert_eq!(c.rental_rows()?[0].status.as_str(), "Reservada");

    c.rental_pick_up(&rental_id)?;
    c.rental_register_return(&rental_id, 0, "2", "0", "0", "")?;
    assert_eq!(c.rental_rows()?[0].status.as_str(), "Retirada");

    c.rental_register_return(&rental_id, 0, "0", "1", "0", "avaria leve")?;
    assert_eq!(c.rental_rows()?[0].status.as_str(), "Encerrada");
    Ok(())
}

#[test]
fn rental_payment_settles_the_balance() -> TestResult {
    let mut c = controller()?;
    seed(&mut c, "10")?;
    let lines = draft(&mut c, "3")?;
    c.create_rental(rental_form(), &lines)?;
    let rental_id = c.rental_rows()?[0].id.to_string();

    c.rental_pay(&rental_id, 0, "15,00", "")?;

    let view = c.load_rental(&rental_id)?;
    assert!(view.summary.contains("Saldo: R$ 0,00"));
    Ok(())
}

#[test]
fn delivery_moves_from_confirmed_order_to_completion() -> TestResult {
    let mut c = controller()?;
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
        mode_index: 0,
        street: "Rua das Flores, 10".into(),
        neighborhood: "Centro".into(),
        city: "Fortaleza".into(),
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

    assert_eq!(c.delivery_order_options()?.len(), 1);
    c.create_delivery(NewDeliveryForm {
        order_index: 0,
        fee: "8,00".into(),
        scheduled: "10/01/2099 09:00".into(),
        ..NewDeliveryForm::default()
    })?;

    c.save_deliverer(DelivererForm {
        name: "Carlos".into(),
        phone: "(85) 98888-7777".into(),
        ..DelivererForm::default()
    })?;

    let rows = c.delivery_rows(0, "")?;
    assert_eq!(rows[0].status.as_str(), "Agendada");
    assert_eq!(rows[0].deliverer.as_str(), "aguardando entregador");
    let delivery_id = rows[0].id.to_string();

    c.assign_delivery(&delivery_id, 0)?;
    c.depart_delivery(&delivery_id)?;
    c.complete_delivery(&delivery_id, "Ana", "")?;
    assert_eq!(c.delivery_rows(0, "")?[0].status.as_str(), "Entregue");

    let sale_id = c.sale_rows("")?[0].id.to_string();
    let err = c.cancel_sale(&sale_id, "cliente desistiu").unwrap_err();
    assert!(err.contains("tratado manualmente"));
    Ok(())
}