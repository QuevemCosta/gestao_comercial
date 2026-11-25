//! Montagem da janela Slint: liga cada componente ao controlador.
//!
//! As guias internas (listagem e formulário) são trocadas aqui, conforme o resultado
//! da ação: a tela só muda de guia quando a operação dá certo, para que o usuário
//! não perca o formulário em caso de erro.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::application::reports::ReportTable;
use crate::infrastructure::database::Database;

use super::controller::logistics::{
    DelivererForm, DeliveryView, DraftLine, NewDeliveryForm, RentalForm, RentalItemForm, RentalView,
};
use super::controller::reports::{report_kind_options, report_row};
use super::controller::{
    Controller, CustomerForm, ItemForm, OrderForm, OrderView, PaymentView, ProductForm, SaleView,
    StockAction,
};
use super::{DraftLineRow, MainWindow};

/// Abre a janela principal e executa o laço de eventos até ela ser fechada.
pub fn run(db: Database) -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let controller = Rc::new(RefCell::new(Controller::new(db)));

    refresh_or_report(&ui, &controller);

    wire_navigation(&ui, &controller);
    wire_products(&ui, &controller);
    wire_categories(&ui, &controller);
    wire_stock(&ui, &controller);
    wire_customers(&ui, &controller);
    wire_orders(&ui, &controller);
    wire_sales(&ui, &controller);
    wire_payments(&ui, &controller);
    wire_rental_items(&ui, &controller);
    wire_rentals(&ui, &controller);
    wire_deliverers(&ui, &controller);
    wire_deliveries(&ui, &controller);
    wire_reports(&ui, &controller);
    wire_settings(&ui, &controller);

    ui.run()
}

// ---------- Navegação ----------

fn wire_navigation(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_navigate(move |key: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        reset_tabs(&ui);
        ui.set_page(page_for(key.as_str()).into());
        refresh_or_report(&ui, &c);
    });
}

// ---------- Produtos ----------

fn wire_products(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_search_products(move || {
        let Some(ui) = weak.upgrade() else { return };
        let search = ui.get_product_search();
        let outcome = c.borrow_mut().product_rows(search.as_str());
        match outcome {
            Ok(rows) => ui.set_products(model(rows)),
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    ui.on_new_product(move || {
        if let Some(ui) = weak.upgrade() {
            clear_product_form(&ui);
            ui.set_product_tab(1);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_product(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_product(id.as_str());
        match outcome {
            Ok(form) => {
                fill_product_form(&ui, &form);
                ui.set_product_stock(product_stock(&c, id.as_str()).into());
                ui.set_product_tab(1);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_save_product(move || {
        let Some(ui) = weak.upgrade() else { return };
        let form = product_form_from_ui(&ui);
        let outcome = c.borrow_mut().save_product(form);
        if outcome.is_ok() {
            clear_product_form(&ui);
            ui.set_product_tab(0);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_toggle_product(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().toggle_product(id.as_str());
        report(&ui, &c, outcome);
    });
}

// ---------- Categorias ----------

fn wire_categories(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    ui.on_new_category(move || {
        if let Some(ui) = weak.upgrade() {
            clear_category_form(&ui);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_category(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let name = c.borrow().category_name(id.as_str());
        match name {
            Some(name) => {
                ui.set_category_id(id);
                ui.set_category_name(name.into());
                show_message(&ui, "", false);
            }
            None => show_message(&ui, "Categoria não encontrada.", true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_save_category(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_category_id();
        let name = ui.get_category_name();
        let outcome = c.borrow_mut().save_category(id.as_str(), name.as_str());
        if outcome.is_ok() {
            clear_category_form(&ui);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_toggle_category(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().toggle_category(id.as_str());
        report(&ui, &c, outcome);
    });
}

// ---------- Estoque ----------

fn wire_stock(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_stock_item(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().select_stock_item(id.as_str());
        match outcome {
            Ok((label, rows)) => {
                ui.set_stock_selected_id(id);
                ui.set_stock_selected_label(label.into());
                ui.set_movements(model(rows));
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_stock_entry(move || {
        if let Some(ui) = weak.upgrade() {
            stock_action(&ui, &c, StockAction::Entry);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_stock_exit(move || {
        if let Some(ui) = weak.upgrade() {
            stock_action(&ui, &c, StockAction::Exit);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_stock_adjust(move || {
        if let Some(ui) = weak.upgrade() {
            stock_action(&ui, &c, StockAction::Adjust);
        }
    });
}

// ---------- Clientes ----------

fn wire_customers(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    ui.on_new_customer(move || {
        if let Some(ui) = weak.upgrade() {
            clear_customer_form(&ui);
            ui.set_customer_tab(1);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_search_customers(move || {
        let Some(ui) = weak.upgrade() else { return };
        let search = ui.get_customer_search();
        let outcome = c.borrow_mut().customer_rows(search.as_str());
        match outcome {
            Ok(rows) => ui.set_customers(model(rows)),
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_customer(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_customer(id.as_str());
        match outcome {
            Ok(form) => {
                fill_customer_form(&ui, &form);
                ui.set_customer_tab(1);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_save_customer(move || {
        let Some(ui) = weak.upgrade() else { return };
        let form = customer_form_from_ui(&ui);
        let outcome = c.borrow_mut().save_customer(form);
        if outcome.is_ok() {
            clear_customer_form(&ui);
            ui.set_customer_tab(0);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_toggle_customer(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().toggle_customer(id.as_str());
        report(&ui, &c, outcome);
    });
}

// ---------- Pedidos ----------

fn wire_orders(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    ui.on_new_order(move || {
        if let Some(ui) = weak.upgrade() {
            clear_order_form(&ui);
            ui.set_order_tab(1);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_order(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_order(id.as_str());
        match outcome {
            Ok(view) => {
                fill_order_form(&ui, &view.form);
                set_order_state(&ui, &view);
                ui.set_order_tab(1);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_save_order(move || {
        let Some(ui) = weak.upgrade() else { return };
        let was_new = ui.get_order_id().is_empty();
        let form = order_form_from_ui(&ui);
        let outcome = c.borrow_mut().save_order(form);
        match outcome {
            Ok(id) => {
                ui.set_order_id(id.as_str().into());
                let text = if was_new {
                    "Pedido criado. Agora adicione os itens."
                } else {
                    "Alterações do pedido salvas."
                };
                report(&ui, &c, Ok(text.to_owned()));
            }
            Err(error) => report(&ui, &c, Err(error)),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_add_order_item(move || {
        let Some(ui) = weak.upgrade() else { return };
        let order_id = ui.get_order_id();
        let item = item_form_from_ui(&ui);
        let outcome = c
            .borrow_mut()
            .add_order_item(order_id.as_str(), item)
            .map(|()| "Item incluído.".to_owned());
        if outcome.is_ok() {
            clear_item_form(&ui);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_remove_order_item(move |position: i32| {
        let Some(ui) = weak.upgrade() else { return };
        let order_id = ui.get_order_id();
        let outcome = c
            .borrow_mut()
            .remove_order_item(order_id.as_str(), position)
            .map(|()| "Item removido.".to_owned());
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_confirm_order(move || {
        let Some(ui) = weak.upgrade() else { return };
        let order_id = ui.get_order_id();
        let outcome = c.borrow_mut().confirm_order(order_id.as_str());
        if outcome.is_ok() {
            ui.set_order_tab(0);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_cancel_order(move || {
        let Some(ui) = weak.upgrade() else { return };
        let order_id = ui.get_order_id();
        let outcome = c.borrow_mut().cancel_order(order_id.as_str());
        if outcome.is_ok() {
            ui.set_order_tab(0);
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Vendas ----------

fn wire_sales(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_search_sales(move || {
        let Some(ui) = weak.upgrade() else { return };
        let number = ui.get_sale_search();
        let outcome = c.borrow_mut().sale_rows(number.as_str());
        match outcome {
            Ok(rows) => {
                ui.set_sales(model(rows));
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_sale(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_sale(id.as_str());
        match outcome {
            Ok(view) => {
                ui.set_sale_id(id);
                set_sale_state(&ui, &view);
                ui.set_sale_tab(1);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_complete_sale(move || {
        let Some(ui) = weak.upgrade() else { return };
        let sale_id = ui.get_sale_id();
        let outcome = c.borrow_mut().complete_sale(sale_id.as_str());
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_cancel_sale(move || {
        let Some(ui) = weak.upgrade() else { return };
        let sale_id = ui.get_sale_id();
        let reason = ui.get_sale_cancel_reason();
        let outcome = c.borrow_mut().cancel_sale(sale_id.as_str(), reason.as_str());
        if outcome.is_ok() {
            ui.set_sale_cancel_reason("".into());
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Pagamentos ----------

fn wire_payments(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_payment_sale(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_payment_view(id.as_str());
        match outcome {
            Ok(view) => {
                ui.set_pay_sale_id(id);
                set_pay_state(&ui, &view);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_register_payment(move || {
        let Some(ui) = weak.upgrade() else { return };
        let sale_id = ui.get_pay_sale_id();
        let amount = ui.get_pay_amount();
        let method = ui.get_pay_method_index();
        let notes = ui.get_pay_notes();
        let outcome = c.borrow_mut().register_payment(
            sale_id.as_str(),
            method,
            amount.as_str(),
            notes.as_str(),
        );
        if outcome.is_ok() {
            ui.set_pay_amount("".into());
            ui.set_pay_notes("".into());
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Itens de locação ----------

fn wire_rental_items(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    ui.on_new_rental_item(move || {
        if let Some(ui) = weak.upgrade() {
            clear_rental_item_form(&ui);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_rental_item(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_rental_item(id.as_str());
        match outcome {
            Ok(form) => {
                fill_rental_item_form(&ui, &form);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_save_rental_item(move || {
        let Some(ui) = weak.upgrade() else { return };
        let form = rental_item_form_from_ui(&ui);
        let outcome = c.borrow_mut().save_rental_item(form);
        if outcome.is_ok() {
            clear_rental_item_form(&ui);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_toggle_rental_item(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().toggle_rental_item(id.as_str());
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_add_rental_units(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_rental_item_id();
        let units = ui.get_rental_item_units();
        let reason = ui.get_rental_item_reason();
        let outcome = c.borrow_mut().change_rental_units(
            id.as_str(),
            units.as_str(),
            reason.as_str(),
            true,
        );
        if outcome.is_ok() {
            ui.set_rental_item_units("".into());
            ui.set_rental_item_reason("".into());
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_write_off_rental_units(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_rental_item_id();
        let units = ui.get_rental_item_units();
        let reason = ui.get_rental_item_reason();
        let outcome = c.borrow_mut().change_rental_units(
            id.as_str(),
            units.as_str(),
            reason.as_str(),
            false,
        );
        if outcome.is_ok() {
            ui.set_rental_item_units("".into());
            ui.set_rental_item_reason("".into());
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Locações ----------

fn wire_rentals(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    ui.on_new_rental(move || {
        if let Some(ui) = weak.upgrade() {
            clear_rental_form(&ui);
            ui.set_rental_tab(1);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_rental(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_rental(id.as_str());
        match outcome {
            Ok(view) => {
                ui.set_rental_id(id);
                set_rental_view(&ui, &view);
                ui.set_rental_tab(1);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_rental_draft_add(move || {
        let Some(ui) = weak.upgrade() else { return };
        let item_index = ui.get_rental_draft_item_index();
        let quantity = ui.get_rental_draft_quantity();
        let price = ui.get_rental_draft_price();
        let outcome = c
            .borrow_mut()
            .rental_draft_line(item_index, quantity.as_str(), price.as_str());
        match outcome {
            Ok(mut row) => {
                let mut rows: Vec<DraftLineRow> = ui.get_rental_draft().iter().collect();
                row.index = rows.len() as i32;
                rows.push(row);
                ui.set_rental_draft(model(rows));
                ui.set_rental_draft_price("".into());
                show_message(&ui, "Item incluído na locação.", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    ui.on_rental_draft_remove(move |index: i32| {
        if let Some(ui) = weak.upgrade() {
            let rows: Vec<DraftLineRow> = ui
                .get_rental_draft()
                .iter()
                .filter(|row| row.index != index)
                .collect();
            ui.set_rental_draft(model(reindex(rows)));
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_rental_create(move || {
        let Some(ui) = weak.upgrade() else { return };
        let form = rental_form_from_ui(&ui);
        let lines: Vec<DraftLine> = ui
            .get_rental_draft()
            .iter()
            .map(|row| DraftLine {
                item_id: row.item_id.to_string(),
                quantity: row.quantity.to_string(),
                price: row.price.to_string(),
            })
            .collect();
        let outcome = c.borrow_mut().create_rental(form, &lines);
        if outcome.is_ok() {
            clear_rental_form(&ui);
            ui.set_rental_tab(0);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_rental_pick_up(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_rental_id();
        let outcome = c.borrow_mut().rental_pick_up(id.as_str());
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_rental_cancel(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_rental_id();
        let reason = ui.get_rental_cancel_reason();
        let outcome = c.borrow_mut().rental_cancel(id.as_str(), reason.as_str());
        if outcome.is_ok() {
            ui.set_rental_cancel_reason("".into());
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_rental_register_return(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_rental_id();
        let line_index = ui.get_rental_return_line_index();
        let returned = ui.get_rental_return_returned();
        let damaged = ui.get_rental_return_damaged();
        let lost = ui.get_rental_return_lost();
        let notes = ui.get_rental_return_notes();
        let outcome = c.borrow_mut().rental_register_return(
            id.as_str(),
            line_index,
            returned.as_str(),
            damaged.as_str(),
            lost.as_str(),
            notes.as_str(),
        );
        if outcome.is_ok() {
            ui.set_rental_return_returned("0".into());
            ui.set_rental_return_damaged("0".into());
            ui.set_rental_return_lost("0".into());
            ui.set_rental_return_notes("".into());
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_rental_pay(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_rental_id();
        let method = ui.get_rental_pay_method_index();
        let amount = ui.get_rental_pay_amount();
        let notes = ui.get_rental_pay_notes();
        let outcome = c.borrow_mut().rental_pay(
            id.as_str(),
            method,
            amount.as_str(),
            notes.as_str(),
        );
        if outcome.is_ok() {
            ui.set_rental_pay_amount("".into());
            ui.set_rental_pay_notes("".into());
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Entregadores ----------

fn wire_deliverers(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    ui.on_new_deliverer(move || {
        if let Some(ui) = weak.upgrade() {
            clear_deliverer_form(&ui);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_deliverer(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_deliverer(id.as_str());
        match outcome {
            Ok(form) => {
                fill_deliverer_form(&ui, &form);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_save_deliverer(move || {
        let Some(ui) = weak.upgrade() else { return };
        let form = deliverer_form_from_ui(&ui);
        let outcome = c.borrow_mut().save_deliverer(form);
        if outcome.is_ok() {
            clear_deliverer_form(&ui);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_toggle_deliverer(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().toggle_deliverer(id.as_str());
        report(&ui, &c, outcome);
    });
}

// ---------- Entregas ----------

fn wire_deliveries(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_search_deliveries(move || {
        if let Some(ui) = weak.upgrade() {
            report(&ui, &c, Ok("Lista atualizada.".to_owned()));
        }
    });

    let weak = ui.as_weak();
    ui.on_new_delivery(move || {
        if let Some(ui) = weak.upgrade() {
            clear_delivery_selection(&ui);
            clear_new_delivery_form(&ui);
            show_message(&ui, "", false);
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_select_delivery(move |id: SharedString| {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().load_delivery(id.as_str());
        match outcome {
            Ok(view) => {
                ui.set_delivery_id(id);
                set_delivery_view(&ui, &view);
                show_message(&ui, "", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_create_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let form = new_delivery_form_from_ui(&ui);
        let outcome = c.borrow_mut().create_delivery(form);
        if outcome.is_ok() {
            clear_new_delivery_form(&ui);
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_schedule_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_delivery_id();
        let when = ui.get_delivery_schedule_text();
        let outcome = c.borrow_mut().schedule_delivery(id.as_str(), when.as_str());
        if outcome.is_ok() {
            ui.set_delivery_schedule_text("".into());
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_assign_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_delivery_id();
        let index = ui.get_delivery_deliverer_index();
        let outcome = c.borrow_mut().assign_delivery(id.as_str(), index);
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_depart_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_delivery_id();
        let outcome = c.borrow_mut().depart_delivery(id.as_str());
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_complete_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_delivery_id();
        let received = ui.get_delivery_received_by();
        let notes = ui.get_delivery_note();
        let outcome = c.borrow_mut().complete_delivery(
            id.as_str(),
            received.as_str(),
            notes.as_str(),
        );
        if outcome.is_ok() {
            ui.set_delivery_received_by("".into());
            ui.set_delivery_note("".into());
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_fail_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_delivery_id();
        let reason = ui.get_delivery_reason();
        let outcome = c.borrow_mut().fail_delivery(id.as_str(), reason.as_str());
        if outcome.is_ok() {
            ui.set_delivery_reason("".into());
        }
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_cancel_delivery(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_delivery_id();
        let reason = ui.get_delivery_reason();
        let outcome = c.borrow_mut().cancel_delivery(id.as_str(), reason.as_str());
        if outcome.is_ok() {
            ui.set_delivery_reason("".into());
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Relatórios ----------

fn wire_reports(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_generate_report(move || {
        let Some(ui) = weak.upgrade() else { return };
        let kind = ui.get_report_kind_index();
        let from = ui.get_report_from();
        let to = ui.get_report_to();
        let outcome = c.borrow_mut().report_table(kind, from.as_str(), to.as_str());
        match outcome {
            Ok(table) => {
                apply_table(&ui, &table);
                show_message(&ui, "Relatório gerado.", false);
            }
            Err(error) => show_message(&ui, &error, true),
        }
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_export_report(move || {
        let Some(ui) = weak.upgrade() else { return };
        let kind = ui.get_report_kind_index();
        let from = ui.get_report_from();
        let to = ui.get_report_to();
        let outcome = c.borrow_mut().export_report(kind, from.as_str(), to.as_str());
        report(&ui, &c, outcome);
    });
}

// ---------- Configurações ----------

fn wire_settings(ui: &MainWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_create_backup(move || {
        let Some(ui) = weak.upgrade() else { return };
        let outcome = c.borrow_mut().create_backup();
        report(&ui, &c, outcome);
    });

    let weak = ui.as_weak();
    let c = Rc::clone(controller);
    ui.on_prepare_restore(move || {
        let Some(ui) = weak.upgrade() else { return };
        let index = ui.get_settings_backup_index();
        let confirmed = ui.get_settings_restore_confirmed();
        let outcome = c.borrow_mut().prepare_restore(index, confirmed);
        if outcome.is_ok() {
            ui.set_settings_restore_confirmed(false);
        }
        report(&ui, &c, outcome);
    });
}

// ---------- Atualização e mensagens ----------

fn stock_action(ui: &MainWindow, controller: &RefCell<Controller>, action: StockAction) {
    let selected = ui.get_stock_selected_id();
    let amount = ui.get_stock_amount();
    let reason = ui.get_stock_reason();
    let outcome = controller.borrow_mut().stock_operation(
        selected.as_str(),
        action,
        amount.as_str(),
        reason.as_str(),
    );
    if outcome.is_ok() {
        ui.set_stock_amount("".into());
        ui.set_stock_reason("".into());
    }
    report(ui, controller, outcome);
}

/// Recarrega os dados exibidos a partir do controlador.
/// Campos de formulário em edição não são sobrescritos: só listas e estados derivados.
fn refresh_all(ui: &MainWindow, controller: &RefCell<Controller>) -> Result<(), String> {
    let mut c = controller.borrow_mut();

    c.load_categories()?;
    c.load_lookups()?;
    ui.set_category_options(model(c.category_options()));
    ui.set_categories(model(c.category_rows()));
    ui.set_customer_options(model(c.customer_options()));
    ui.set_product_options(model(c.product_options()));

    ui.set_dash_cards(model(c.dashboard()?));

    let search = ui.get_product_search();
    ui.set_products(model(c.product_rows(search.as_str())?));

    ui.set_stock_items(model(c.stock_items()?));
    ui.set_stock_alert(c.stock_alert_text()?.into());

    let selected = ui.get_stock_selected_id();
    if !selected.is_empty() {
        ui.set_movements(model(c.stock_movements(selected.as_str())?));
    }

    let product_id = ui.get_product_id();
    if !product_id.is_empty() {
        let stock = c
            .stock_items()?
            .into_iter()
            .find(|row| row.id == product_id)
            .map(|row| row.stock.as_str().to_owned())
            .unwrap_or_default();
        ui.set_product_stock(stock.into());
    }

    let customer_search = ui.get_customer_search();
    ui.set_customers(model(c.customer_rows(customer_search.as_str())?));

    ui.set_orders(model(c.order_rows()?));
    let order_id = ui.get_order_id();
    if !order_id.is_empty() {
        let view = c.load_order(order_id.as_str())?;
        set_order_state(ui, &view);
    }

    let number = ui.get_sale_search();
    ui.set_sales(model(c.sale_rows(number.as_str())?));
    let sale_id = ui.get_sale_id();
    if !sale_id.is_empty() {
        let view = c.load_sale(sale_id.as_str())?;
        set_sale_state(ui, &view);
    }

    ui.set_balances(model(c.balance_rows()?));
    let pay_id = ui.get_pay_sale_id();
    if !pay_id.is_empty() {
        let view = c.load_payment_view(pay_id.as_str())?;
        set_pay_state(ui, &view);
    }

    ui.set_rental_items(model(c.rental_item_rows()?));
    ui.set_rental_item_options(model(c.rental_item_options()?));
    ui.set_rentals(model(c.rental_rows()?));
    let rental_id = ui.get_rental_id();
    if !rental_id.is_empty() {
        let view = c.load_rental(rental_id.as_str())?;
        set_rental_view(ui, &view);
    }

    ui.set_deliverers(model(c.deliverer_rows()?));
    ui.set_deliverer_options(model(c.deliverer_options()?));
    ui.set_delivery_order_options(model(c.delivery_order_options()?));
    ui.set_delivery_panel(model(c.delivery_panel_cards()?));
    let view_index = ui.get_delivery_view_index();
    let date = ui.get_delivery_date();
    ui.set_deliveries(model(c.delivery_rows(view_index, date.as_str())?));
    let delivery_id = ui.get_delivery_id();
    if !delivery_id.is_empty() {
        let view = c.load_delivery(delivery_id.as_str())?;
        set_delivery_view(ui, &view);
    }

    ui.set_report_kind_options(model(report_kind_options()));

    let settings = c.settings_view()?;
    ui.set_settings_data_folder(settings.data_folder.as_str().into());
    ui.set_settings_database_file(settings.database_file.as_str().into());
    ui.set_settings_backups_folder(settings.backups_folder.as_str().into());
    ui.set_settings_exports_folder(settings.exports_folder.as_str().into());
    ui.set_settings_schema_version(settings.schema_version.as_str().into());
    ui.set_settings_backups(model(settings.backup_options));

    Ok(())
}

fn refresh_or_report(ui: &MainWindow, controller: &RefCell<Controller>) {
    match refresh_all(ui, controller) {
        Ok(()) => show_message(ui, "", false),
        Err(error) => show_message(ui, &error, true),
    }
}

/// Exibe o resultado de uma ação e, se ela deu certo, recarrega os dados.
fn report(ui: &MainWindow, controller: &RefCell<Controller>, outcome: Result<String, String>) {
    match outcome {
        Ok(text) => match refresh_all(ui, controller) {
            Ok(()) => show_message(ui, &text, false),
            Err(error) => show_message(ui, &error, true),
        },
        Err(error) => show_message(ui, &error, true),
    }
}

fn show_message(ui: &MainWindow, text: &str, is_error: bool) {
    ui.set_message(text.into());
    ui.set_message_is_error(is_error);
}

fn reset_tabs(ui: &MainWindow) {
    ui.set_product_tab(0);
    ui.set_customer_tab(0);
    ui.set_order_tab(0);
    ui.set_sale_tab(0);
    ui.set_rental_tab(0);
}

fn page_for(key: &str) -> &'static str {
    match key {
        "dashboard" => "dashboard",
        "products" => "products",
        "categories" => "categories",
        "stock" => "stock",
        "customers" => "customers",
        "orders" => "orders",
        "sales" => "sales",
        "payments" => "payments",
        "rental-items" => "rental-items",
        "rentals" => "rentals",
        "deliverers" => "deliverers",
        "deliveries" => "deliveries",
        "reports" => "reports",
        "settings" => "settings",
        _ => "pending",
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

/// Renumera as linhas do rascunho após uma remoção.
fn reindex(mut rows: Vec<DraftLineRow>) -> Vec<DraftLineRow> {
    for (position, row) in rows.iter_mut().enumerate() {
        row.index = position as i32;
    }
    rows
}

fn apply_table(ui: &MainWindow, table: &ReportTable) {
    ui.set_report_title(table.title.as_str().into());
    ui.set_report_summary(table.summary.as_str().into());
    ui.set_report_header(model(vec![report_row(&table.headers)]));
    ui.set_report_rows(model(table.rows.iter().map(|row| report_row(row)).collect()));
}

/// Saldo atual do produto, lido da lista de estoque. Vazio se não encontrado.
fn product_stock(controller: &RefCell<Controller>, id: &str) -> String {
    let rows = controller.borrow_mut().stock_items().unwrap_or_default();
    rows.into_iter()
        .find(|row| row.id.as_str() == id)
        .map(|row| row.stock.as_str().to_owned())
        .unwrap_or_default()
}

// ---------- Produtos e categorias ----------

fn product_form_from_ui(ui: &MainWindow) -> ProductForm {
    ProductForm {
        id: ui.get_product_id().to_string(),
        sku: ui.get_product_sku().to_string(),
        name: ui.get_product_name().to_string(),
        description: ui.get_product_description().to_string(),
        purchase: ui.get_product_purchase().to_string(),
        sale: ui.get_product_sale().to_string(),
        min_stock: ui.get_product_min_stock().to_string(),
        unit: ui.get_product_unit().to_string(),
        category_index: ui.get_product_category_index(),
    }
}

fn fill_product_form(ui: &MainWindow, form: &ProductForm) {
    ui.set_product_id(form.id.as_str().into());
    ui.set_product_sku(form.sku.as_str().into());
    ui.set_product_name(form.name.as_str().into());
    ui.set_product_description(form.description.as_str().into());
    ui.set_product_purchase(form.purchase.as_str().into());
    ui.set_product_sale(form.sale.as_str().into());
    ui.set_product_min_stock(form.min_stock.as_str().into());
    ui.set_product_unit(form.unit.as_str().into());
    ui.set_product_category_index(form.category_index);
}

fn clear_product_form(ui: &MainWindow) {
    fill_product_form(
        ui,
        &ProductForm {
            min_stock: "0".to_owned(),
            unit: "un".to_owned(),
            ..ProductForm::default()
        },
    );
    ui.set_product_stock("".into());
}

fn clear_category_form(ui: &MainWindow) {
    ui.set_category_id("".into());
    ui.set_category_name("".into());
}

// ---------- Clientes ----------

fn customer_form_from_ui(ui: &MainWindow) -> CustomerForm {
    CustomerForm {
        id: ui.get_customer_id().to_string(),
        name: ui.get_customer_name().to_string(),
        person_index: ui.get_customer_person_index(),
        document: ui.get_customer_document().to_string(),
        phone: ui.get_customer_phone().to_string(),
        whatsapp: ui.get_customer_whatsapp().to_string(),
        street: ui.get_customer_street().to_string(),
        neighborhood: ui.get_customer_neighborhood().to_string(),
        city: ui.get_customer_city().to_string(),
        notes: ui.get_customer_notes().to_string(),
    }
}

fn fill_customer_form(ui: &MainWindow, form: &CustomerForm) {
    ui.set_customer_id(form.id.as_str().into());
    ui.set_customer_name(form.name.as_str().into());
    ui.set_customer_person_index(form.person_index);
    ui.set_customer_document(form.document.as_str().into());
    ui.set_customer_phone(form.phone.as_str().into());
    ui.set_customer_whatsapp(form.whatsapp.as_str().into());
    ui.set_customer_street(form.street.as_str().into());
    ui.set_customer_neighborhood(form.neighborhood.as_str().into());
    ui.set_customer_city(form.city.as_str().into());
    ui.set_customer_notes(form.notes.as_str().into());
}

fn clear_customer_form(ui: &MainWindow) {
    fill_customer_form(ui, &CustomerForm::default());
}

// ---------- Pedidos ----------

fn order_form_from_ui(ui: &MainWindow) -> OrderForm {
    OrderForm {
        id: ui.get_order_id().to_string(),
        customer_index: ui.get_order_customer_index(),
        origin_index: ui.get_order_origin_index(),
        mode_index: ui.get_order_mode_index(),
        street: ui.get_order_street().to_string(),
        neighborhood: ui.get_order_neighborhood().to_string(),
        city: ui.get_order_city().to_string(),
        discount: ui.get_order_discount().to_string(),
        notes: ui.get_order_notes().to_string(),
    }
}

fn fill_order_form(ui: &MainWindow, form: &OrderForm) {
    ui.set_order_id(form.id.as_str().into());
    ui.set_order_customer_index(form.customer_index);
    ui.set_order_origin_index(form.origin_index);
    ui.set_order_mode_index(form.mode_index);
    ui.set_order_street(form.street.as_str().into());
    ui.set_order_neighborhood(form.neighborhood.as_str().into());
    ui.set_order_city(form.city.as_str().into());
    ui.set_order_discount(form.discount.as_str().into());
    ui.set_order_notes(form.notes.as_str().into());
}

/// Atualiza somente o estado derivado do pedido (itens, totais, situação e edição).
fn set_order_state(ui: &MainWindow, view: &OrderView) {
    ui.set_order_status(view.status.as_str().into());
    ui.set_order_totals(view.totals.as_str().into());
    ui.set_order_items(model(view.items.clone()));
    ui.set_order_editable(view.editable);
}

fn clear_order_form(ui: &MainWindow) {
    fill_order_form(
        ui,
        &OrderForm {
            customer_index: -1,
            discount: "0,00".to_owned(),
            ..OrderForm::default()
        },
    );
    ui.set_order_status("Novo pedido".into());
    ui.set_order_totals("".into());
    ui.set_order_items(model(Vec::new()));
    ui.set_order_editable(true);
    clear_item_form(ui);
}

fn item_form_from_ui(ui: &MainWindow) -> ItemForm {
    ItemForm {
        product_index: ui.get_order_item_product_index(),
        quantity: ui.get_order_item_quantity().to_string(),
        unit_price: ui.get_order_item_price().to_string(),
        discount: ui.get_order_item_discount().to_string(),
    }
}

fn clear_item_form(ui: &MainWindow) {
    ui.set_order_item_product_index(-1);
    ui.set_order_item_quantity("1".into());
    ui.set_order_item_price("".into());
    ui.set_order_item_discount("0,00".into());
}

// ---------- Vendas e pagamentos ----------

fn set_sale_state(ui: &MainWindow, view: &SaleView) {
    ui.set_sale_title(view.title.as_str().into());
    ui.set_sale_totals(view.totals.as_str().into());
    ui.set_sale_items(model(view.items.clone()));
    ui.set_sale_payments(model(view.payments.clone()));
}

fn set_pay_state(ui: &MainWindow, view: &PaymentView) {
    ui.set_pay_title(view.title.as_str().into());
    ui.set_pay_summary(view.summary.as_str().into());
    ui.set_pay_history(model(view.history.clone()));
}

// ---------- Itens de locação ----------

fn rental_item_form_from_ui(ui: &MainWindow) -> RentalItemForm {
    RentalItemForm {
        id: ui.get_rental_item_id().to_string(),
        name: ui.get_rental_item_name().to_string(),
        description: ui.get_rental_item_description().to_string(),
        category: ui.get_rental_item_category().to_string(),
        price: ui.get_rental_item_price().to_string(),
        notes: ui.get_rental_item_notes().to_string(),
        initial_total: ui.get_rental_item_initial().to_string(),
    }
}

fn fill_rental_item_form(ui: &MainWindow, form: &RentalItemForm) {
    ui.set_rental_item_id(form.id.as_str().into());
    ui.set_rental_item_name(form.name.as_str().into());
    ui.set_rental_item_description(form.description.as_str().into());
    ui.set_rental_item_category(form.category.as_str().into());
    ui.set_rental_item_price(form.price.as_str().into());
    ui.set_rental_item_notes(form.notes.as_str().into());
    ui.set_rental_item_initial(form.initial_total.as_str().into());
}

fn clear_rental_item_form(ui: &MainWindow) {
    fill_rental_item_form(
        ui,
        &RentalItemForm {
            initial_total: "0".to_owned(),
            ..RentalItemForm::default()
        },
    );
    ui.set_rental_item_units("".into());
    ui.set_rental_item_reason("".into());
}

// ---------- Locações ----------

fn rental_form_from_ui(ui: &MainWindow) -> RentalForm {
    RentalForm {
        customer_index: ui.get_rental_customer_index(),
        pickup: ui.get_rental_pickup().to_string(),
        expected: ui.get_rental_expected().to_string(),
        discount: ui.get_rental_discount().to_string(),
    }
}

fn set_rental_view(ui: &MainWindow, view: &RentalView) {
    ui.set_rental_title(view.title.as_str().into());
    ui.set_rental_summary(view.summary.as_str().into());
    ui.set_rental_lines(model(view.lines.clone()));
    ui.set_rental_line_options(model(view.line_options.clone()));
}

fn clear_rental_form(ui: &MainWindow) {
    ui.set_rental_id("".into());
    ui.set_rental_title("Nova locação".into());
    ui.set_rental_summary("".into());
    ui.set_rental_lines(model(Vec::new()));
    ui.set_rental_line_options(model(Vec::new()));
    ui.set_rental_customer_index(-1);
    ui.set_rental_pickup("".into());
    ui.set_rental_expected("".into());
    ui.set_rental_discount("0,00".into());
    ui.set_rental_draft(model(Vec::new()));
    ui.set_rental_draft_item_index(-1);
    ui.set_rental_draft_quantity("1".into());
    ui.set_rental_draft_price("".into());
    ui.set_rental_cancel_reason("".into());
    ui.set_rental_return_line_index(-1);
    ui.set_rental_return_returned("0".into());
    ui.set_rental_return_damaged("0".into());
    ui.set_rental_return_lost("0".into());
    ui.set_rental_return_notes("".into());
    ui.set_rental_pay_amount("".into());
    ui.set_rental_pay_notes("".into());
}

// ---------- Entregadores ----------

fn deliverer_form_from_ui(ui: &MainWindow) -> DelivererForm {
    DelivererForm {
        id: ui.get_deliverer_id().to_string(),
        name: ui.get_deliverer_name().to_string(),
        phone: ui.get_deliverer_phone().to_string(),
        document: ui.get_deliverer_document().to_string(),
        notes: ui.get_deliverer_notes().to_string(),
    }
}

fn fill_deliverer_form(ui: &MainWindow, form: &DelivererForm) {
    ui.set_deliverer_id(form.id.as_str().into());
    ui.set_deliverer_name(form.name.as_str().into());
    ui.set_deliverer_phone(form.phone.as_str().into());
    ui.set_deliverer_document(form.document.as_str().into());
    ui.set_deliverer_notes(form.notes.as_str().into());
}

fn clear_deliverer_form(ui: &MainWindow) {
    fill_deliverer_form(ui, &DelivererForm::default());
}

// ---------- Entregas ----------

fn new_delivery_form_from_ui(ui: &MainWindow) -> NewDeliveryForm {
    NewDeliveryForm {
        order_index: ui.get_delivery_new_order_index(),
        phone: ui.get_delivery_new_phone().to_string(),
        fee: ui.get_delivery_new_fee().to_string(),
        notes: ui.get_delivery_new_notes().to_string(),
        scheduled: ui.get_delivery_new_schedule().to_string(),
    }
}

fn clear_new_delivery_form(ui: &MainWindow) {
    ui.set_delivery_new_order_index(-1);
    ui.set_delivery_new_phone("".into());
    ui.set_delivery_new_fee("0,00".into());
    ui.set_delivery_new_notes("".into());
    ui.set_delivery_new_schedule("".into());
}

fn set_delivery_view(ui: &MainWindow, view: &DeliveryView) {
    ui.set_delivery_title(view.title.as_str().into());
    ui.set_delivery_details(view.details.as_str().into());
    ui.set_delivery_history(model(view.history.clone()));
}

fn clear_delivery_selection(ui: &MainWindow) {
    ui.set_delivery_id("".into());
    ui.set_delivery_title("Nova entrega".into());
    ui.set_delivery_details("".into());
    ui.set_delivery_history(model(Vec::new()));
    ui.set_delivery_schedule_text("".into());
    ui.set_delivery_deliverer_index(-1);
    ui.set_delivery_received_by("".into());
    ui.set_delivery_note("".into());
    ui.set_delivery_reason("".into());
}