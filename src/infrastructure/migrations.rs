use rusqlite::{Connection, TransactionBehavior};
use tracing::info;

use super::error::StorageError;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

/// Lista ordenada e imutável. Migrations já aplicadas não devem ser editadas;
/// mudanças são feitas sempre em um arquivo novo.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: include_str!("../../migrations/001_initial_schema.sql"),
    },
    Migration {
        version: 2,
        name: "products_and_stock",
        sql: include_str!("../../migrations/002_products_stock.sql"),
    },
    Migration {
        version: 3,
        name: "customers_and_orders",
        sql: include_str!("../../migrations/003_customers_orders.sql"),
    },
    Migration {
        version: 4,
        name: "sales_and_payments",
        sql: include_str!("../../migrations/004_sales_payments.sql"),
    },
    Migration {
        version: 5,
        name: "rentals",
        sql: include_str!("../../migrations/005_rentals.sql"),
    },
    Migration {
        version: 6,
        name: "deliveries",
        sql: include_str!("../../migrations/006_deliveries.sql"),
    },
];

/// Versão mais alta que esta build sabe aplicar.
pub const CURRENT_VERSION: i64 = MIGRATIONS.len() as i64;

/// Aplica as migrations pendentes. Cada uma roda em sua própria transação,
/// junto com a atualização de `user_version`.
pub fn apply(conn: &mut Connection) -> Result<(), StorageError> {
    let current = current_version(conn)?;

    if current > CURRENT_VERSION {
        return Err(StorageError::SchemaTooNew {
            found: current,
            supported: CURRENT_VERSION,
        });
    }

    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        apply_one(conn, migration)?;
    }

    Ok(())
}

pub fn current_version(conn: &Connection) -> Result<i64, StorageError> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn apply_one(conn: &mut Connection, migration: &Migration) -> Result<(), StorageError> {
    info!(version = migration.version, name = migration.name, "aplicando migração");

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(migration.sql).map_err(|source| StorageError::Migration {
        version: migration.version,
        source,
    })?;
    tx.pragma_update(None, "user_version", migration.version)?;
    tx.commit()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_sequential_starting_at_one() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version, index as i64 + 1);
        }
    }
}