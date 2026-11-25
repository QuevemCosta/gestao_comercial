use uuid::Uuid;

use crate::domain::customer::{Customer, CustomerData};
use crate::domain::repositories::CustomerFilter;

use super::clock::Clock;
use super::error::AppError;
use super::unit_of_work::{Transactor, UnitOfWork};

/// Casos de uso de clientes.
pub struct Customers<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Customers<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Customers<'_, S> {
    pub fn register(&mut self, data: CustomerData) -> Result<Customer, AppError> {
        let customer = Customer::create(Uuid::now_v7(), data, self.clock.now())?;
        let mut uow = self.store.begin()?;
        uow.insert_customer(&customer)?;
        uow.commit()?;
        Ok(customer)
    }

    pub fn update(&mut self, id: Uuid, data: CustomerData) -> Result<Customer, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut customer = require_customer(&*uow, id)?;
        customer.update(data, now)?;
        uow.update_customer(&customer)?;
        uow.commit()?;
        Ok(customer)
    }

    pub fn set_active(&mut self, id: Uuid, active: bool) -> Result<Customer, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut customer = require_customer(&*uow, id)?;
        customer.set_active(active, now);
        uow.update_customer(&customer)?;
        uow.commit()?;
        Ok(customer)
    }

    pub fn get(&mut self, id: Uuid) -> Result<Customer, AppError> {
        let uow = self.store.begin()?;
        let customer = require_customer(&*uow, id)?;
        uow.commit()?;
        Ok(customer)
    }

    pub fn search(&mut self, filter: &CustomerFilter) -> Result<Vec<Customer>, AppError> {
        let uow = self.store.begin()?;
        let customers = uow.search_customers(filter)?;
        uow.commit()?;
        Ok(customers)
    }
}

fn require_customer(uow: &dyn UnitOfWork, id: Uuid) -> Result<Customer, AppError> {
    uow.find_customer(id)?
        .ok_or(AppError::NotFound("cliente"))
}