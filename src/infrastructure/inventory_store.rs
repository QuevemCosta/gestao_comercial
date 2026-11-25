use chrono::NaiveDateTime;
use rusqlite::types::Type;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::category::Category;
use crate::domain::errors::RepositoryError;
use crate::domain::product::{Product, ProductData};
use crate::domain::repositories::{
    CategoryRepository, ProductFilter, ProductRepository, StockAlert, StockRepository,
};
use crate::domain::stock::{MovementKind, PlannedMovement, StockMovement};
use crate::domain::text::search_key;
use crate::domain::value_objects::Money;

use super::database::SqliteUnitOfWork;

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

const SELECT_CATEGORY: &str = "SELECT id, name, active, created_at FROM categories";

const SELECT_PRODUCT: &str = "SELECT id, sku, name, description, category_id, \
    purchase_price_cents, sale_price_cents, stock_quantity, min_stock, unit, \
    active, created_at, updated_at FROM products";

/// Implementação SQLite dos repositórios. Todas as operações ocorrem dentro da unidade de trabalho.
impl CategoryRepository for SqliteUnitOfWork<'_> {
    fn insert_category(&mut self, category: &Category) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO categories (id, name, active, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    category.id().to_string(),
                    category.name(),
                    category.is_active(),
                    format_dt(category.created_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_category(&mut self, category: &Category) -> Result<(), RepositoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE categories SET name = ?2, active = ?3 WHERE id = ?1",
                params![category.id().to_string(), category.name(), category.is_active()],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_category(&self, id: Uuid) -> Result<Option<Category>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_CATEGORY} WHERE id = ?1"),
                params![id.to_string()],
                row_to_category,
            )
            .optional()
            .map_err(map_error)
    }

    fn list_categories(&self, only_active: bool) -> Result<Vec<Category>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "{SELECT_CATEGORY} WHERE (?1 = 0 OR active = 1) ORDER BY name COLLATE NOCASE"
            ))
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![only_active], row_to_category)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

impl ProductRepository for SqliteUnitOfWork<'_> {
    fn insert_product(&mut self, product: &Product) -> Result<(), RepositoryError> {
        let data = product.data();
        self.conn()
            .execute(
                "INSERT INTO products (id, sku, name, description, category_id, \
                 purchase_price_cents, sale_price_cents, stock_quantity, min_stock, unit, \
                 active, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    product.id().to_string(),
                    data.sku,
                    data.name,
                    data.description,
                    data.category_id.map(|id| id.to_string()),
                    data.purchase_price.cents(),
                    data.sale_price.cents(),
                    product.stock(),
                    data.min_stock,
                    data.unit,
                    product.is_active(),
                    format_dt(product.created_at()),
                    format_dt(product.updated_at()),
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn update_product(&mut self, product: &Product) -> Result<(), RepositoryError> {
        let data = product.data();
        // stock_quantity não aparece aqui de propósito: o saldo só muda por movimentação.
        let changed = self
            .conn()
            .execute(
                "UPDATE products SET sku = ?2, name = ?3, description = ?4, category_id = ?5, \
                 purchase_price_cents = ?6, sale_price_cents = ?7, min_stock = ?8, unit = ?9, \
                 active = ?10, updated_at = ?11 \
                 WHERE id = ?1",
                params![
                    product.id().to_string(),
                    data.sku,
                    data.name,
                    data.description,
                    data.category_id.map(|id| id.to_string()),
                    data.purchase_price.cents(),
                    data.sale_price.cents(),
                    data.min_stock,
                    data.unit,
                    product.is_active(),
                    format_dt(product.updated_at()),
                ],
            )
            .map_err(map_error)?;
        require_one_row(changed)
    }

    fn find_product(&self, id: Uuid) -> Result<Option<Product>, RepositoryError> {
        self.conn()
            .query_row(
                &format!("{SELECT_PRODUCT} WHERE id = ?1"),
                params![id.to_string()],
                row_to_product,
            )
            .optional()
            .map_err(map_error)
    }

    fn search_products(&self, filter: &ProductFilter) -> Result<Vec<Product>, RepositoryError> {
        // Texto vazio é tratado como ausência de filtro. Wildcards do usuário são escapados.
        let pattern = filter
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| format!("%{}%", escape_like(&search_key(text))));
        let category = filter.category_id.map(|id| id.to_string());
        let alert = match filter.alert {
            None => 0,
            Some(StockAlert::LowStock) => 1,
            Some(StockAlert::OutOfStock) => 2,
        };

        let sql = format!(
            "{SELECT_PRODUCT} \
             WHERE (?1 = 1 OR active = 1) \
               AND (?2 IS NULL OR category_id = ?2) \
               AND (?3 IS NULL \
                    OR search_key(name) LIKE ?3 ESCAPE '\\' \
                    OR search_key(sku) LIKE ?3 ESCAPE '\\') \
               AND (?4 = 0 \
                    OR (?4 = 1 AND stock_quantity <= min_stock) \
                    OR (?4 = 2 AND stock_quantity = 0)) \
             ORDER BY name COLLATE NOCASE"
        );

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(map_error)?;
        let rows = stmt
            .query_map(
                params![filter.include_inactive, category, pattern, alert],
                row_to_product,
            )
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

impl StockRepository for SqliteUnitOfWork<'_> {
    fn record_stock_movement(
        &mut self,
        movement: &StockMovement,
        previous_balance: i64,
    ) -> Result<(), RepositoryError> {
        let conn = self.conn();
        let product_key = movement.product_id().to_string();

        // Proteção extra: o saldo só é trocado se ainda for o lido nesta transação.
        let updated = conn
            .execute(
                "UPDATE products SET stock_quantity = ?2, updated_at = ?3 \
                 WHERE id = ?1 AND stock_quantity = ?4",
                params![
                    product_key,
                    movement.balance_after(),
                    format_dt(movement.occurred_at()),
                    previous_balance,
                ],
            )
            .map_err(map_error)?;

        if updated != 1 {
            return Err(RepositoryError::Conflict(
                "o saldo foi alterado durante a operação; tente novamente".to_owned(),
            ));
        }

        conn.execute(
            "INSERT INTO stock_movements \
             (id, product_id, kind, quantity_delta, balance_after, reason, reverses_movement_id, occurred_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                movement.id().to_string(),
                product_key,
                movement.kind().as_str(),
                movement.delta(),
                movement.balance_after(),
                movement.reason(),
                movement.reverses().map(|id| id.to_string()),
                format_dt(movement.occurred_at()),
            ],
        )
        .map_err(map_error)?;

        Ok(())
    }

    fn movements_for(&self, product_id: Uuid) -> Result<Vec<StockMovement>, RepositoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, product_id, kind, quantity_delta, balance_after, reason, \
                 reverses_movement_id, occurred_at \
                 FROM stock_movements WHERE product_id = ?1 ORDER BY occurred_at, id",
            )
            .map_err(map_error)?;
        let rows = stmt
            .query_map(params![product_id.to_string()], row_to_movement)
            .map_err(map_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_error)
    }
}

fn row_to_category(row: &Row) -> rusqlite::Result<Category> {
    Ok(Category::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        row.get("name")?,
        row.get("active")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
    ))
}

fn row_to_product(row: &Row) -> rusqlite::Result<Product> {
    let category_id: Option<String> = row.get("category_id")?;
    let data = ProductData {
        sku: row.get("sku")?,
        name: row.get("name")?,
        description: row.get("description")?,
        category_id: category_id.as_deref().map(parse_uuid).transpose()?,
        purchase_price: Money::from_cents(row.get("purchase_price_cents")?),
        sale_price: Money::from_cents(row.get("sale_price_cents")?),
        min_stock: row.get("min_stock")?,
        unit: row.get("unit")?,
    };
    Ok(Product::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        data,
        row.get("stock_quantity")?,
        row.get("active")?,
        parse_dt(&row.get::<_, String>("created_at")?)?,
        parse_dt(&row.get::<_, String>("updated_at")?)?,
    ))
}

fn row_to_movement(row: &Row) -> rusqlite::Result<StockMovement> {
    let kind_raw: String = row.get("kind")?;
    let kind = MovementKind::parse(&kind_raw).ok_or_else(|| invalid_value("kind", &kind_raw))?;
    let reverses: Option<String> = row.get("reverses_movement_id")?;

    Ok(StockMovement::restore(
        parse_uuid(&row.get::<_, String>("id")?)?,
        parse_uuid(&row.get::<_, String>("product_id")?)?,
        PlannedMovement {
            kind,
            delta: row.get("quantity_delta")?,
            balance_after: row.get("balance_after")?,
        },
        row.get("reason")?,
        reverses.as_deref().map(parse_uuid).transpose()?,
        parse_dt(&row.get::<_, String>("occurred_at")?)?,
    ))
}

/// Escapa os caracteres especiais do LIKE para que sejam tratados como texto comum.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

fn parse_uuid(raw: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(raw)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

fn parse_dt(raw: &str) -> rusqlite::Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(raw, DATE_FORMAT)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

fn format_dt(value: NaiveDateTime) -> String {
    value.format(DATE_FORMAT).to_string()
}

fn invalid_value(column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        format!("valor inválido em {column}: {value}").into(),
    )
}

fn require_one_row(changed: usize) -> Result<(), RepositoryError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(RepositoryError::NotFound)
    }
}

/// Converte erros do SQLite em erros de repositório. Violações de unicidade
/// viram `Conflict`, com mensagem compreensível para a interface.
fn map_error(err: rusqlite::Error) -> RepositoryError {
    match err {
        rusqlite::Error::QueryReturnedNoRows => RepositoryError::NotFound,
        rusqlite::Error::SqliteFailure(failure, Some(message))
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
                && message.starts_with("UNIQUE constraint failed") =>
        {
            RepositoryError::Conflict(conflict_message(&message))
        }
        other => RepositoryError::Storage(other.to_string()),
    }
}

fn conflict_message(message: &str) -> String {
    if message.contains("products.sku") {
        "já existe um produto com este SKU".to_owned()
    } else if message.contains("categories.name") {
        "já existe uma categoria com este nome".to_owned()
    } else {
        "registro duplicado".to_owned()
    }
}