use uuid::Uuid;

use crate::domain::category::Category;
use crate::domain::product::{Product, ProductData};
use crate::domain::repositories::ProductFilter;

use super::clock::Clock;
use super::error::AppError;
use super::unit_of_work::{Transactor, UnitOfWork};

/// Casos de uso de cadastro: categorias e produtos.
pub struct Catalog<'a, S> {
    store: &'a mut S,
    clock: &'a dyn Clock,
}

impl<'a, S> Catalog<'a, S> {
    pub fn new(store: &'a mut S, clock: &'a dyn Clock) -> Self {
        Self { store, clock }
    }
}

impl<S: Transactor> Catalog<'_, S> {
    pub fn create_category(&mut self, name: &str) -> Result<Category, AppError> {
        let category = Category::create(Uuid::now_v7(), name, self.clock.now())?;
        let mut uow = self.store.begin()?;
        uow.insert_category(&category)?;
        uow.commit()?;
        Ok(category)
    }

    pub fn rename_category(&mut self, id: Uuid, name: &str) -> Result<Category, AppError> {
        let mut uow = self.store.begin()?;
        let mut category = require_category(&*uow, id)?;
        category.rename(name)?;
        uow.update_category(&category)?;
        uow.commit()?;
        Ok(category)
    }

    pub fn set_category_active(&mut self, id: Uuid, active: bool) -> Result<Category, AppError> {
        let mut uow = self.store.begin()?;
        let mut category = require_category(&*uow, id)?;
        category.set_active(active);
        uow.update_category(&category)?;
        uow.commit()?;
        Ok(category)
    }

    pub fn list_categories(&mut self, only_active: bool) -> Result<Vec<Category>, AppError> {
        let uow = self.store.begin()?;
        let categories = uow.list_categories(only_active)?;
        uow.commit()?;
        Ok(categories)
    }

    pub fn create_product(&mut self, data: ProductData) -> Result<Product, AppError> {
        let now = self.clock.now();
        let category_id = data.category_id;
        let mut uow = self.store.begin()?;
        ensure_category(&*uow, category_id)?;
        let product = Product::create(Uuid::now_v7(), data, now)?;
        uow.insert_product(&product)?;
        uow.commit()?;
        Ok(product)
    }

    pub fn update_product(&mut self, id: Uuid, data: ProductData) -> Result<Product, AppError> {
        let now = self.clock.now();
        let category_id = data.category_id;
        let mut uow = self.store.begin()?;
        let mut product = require_product(&*uow, id)?;
        ensure_category(&*uow, category_id)?;
        product.update(data, now)?;
        uow.update_product(&product)?;
        uow.commit()?;
        Ok(product)
    }

    pub fn set_product_active(&mut self, id: Uuid, active: bool) -> Result<Product, AppError> {
        let now = self.clock.now();
        let mut uow = self.store.begin()?;
        let mut product = require_product(&*uow, id)?;
        product.set_active(active, now);
        uow.update_product(&product)?;
        uow.commit()?;
        Ok(product)
    }

    pub fn get_product(&mut self, id: Uuid) -> Result<Product, AppError> {
        let uow = self.store.begin()?;
        let product = require_product(&*uow, id)?;
        uow.commit()?;
        Ok(product)
    }

    pub fn search_products(&mut self, filter: &ProductFilter) -> Result<Vec<Product>, AppError> {
        let uow = self.store.begin()?;
        let products = uow.search_products(filter)?;
        uow.commit()?;
        Ok(products)
    }
}

fn require_category(uow: &dyn UnitOfWork, id: Uuid) -> Result<Category, AppError> {
    uow.find_category(id)?
        .ok_or(AppError::NotFound("categoria"))
}

fn require_product(uow: &dyn UnitOfWork, id: Uuid) -> Result<Product, AppError> {
    uow.find_product(id)?
        .ok_or(AppError::NotFound("produto"))
}

fn ensure_category(uow: &dyn UnitOfWork, id: Option<Uuid>) -> Result<(), AppError> {
    match id {
        Some(id) => require_category(uow, id).map(|_| ()),
        None => Ok(()),
    }
}