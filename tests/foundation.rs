use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use gestao_comercial::infrastructure::database::Database;
use gestao_comercial::infrastructure::error::StorageError;
use gestao_comercial::infrastructure::migrations::{self, CURRENT_VERSION};

type TestResult = Result<(), Box<dyn Error>>;

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("gc_{name}_{}_{nanos}", std::process::id()))
}

fn insert_audit(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO audit_log (occurred_at, action, entity) VALUES ('2026-01-01T00:00:00', 'teste', 'produto')",
        [],
    )
}

fn count_audit(db: &Database) -> rusqlite::Result<i64> {
    db.connection()
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
}

#[test]
fn in_memory_database_is_migrated_to_current_version() -> TestResult {
    let db = Database::open_in_memory()?;
    assert_eq!(db.schema_version()?, CURRENT_VERSION);
    Ok(())
}

#[test]
fn foreign_keys_are_enabled() -> TestResult {
    let db = Database::open_in_memory()?;
    let enabled: i64 = db
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    assert_eq!(enabled, 1);
    Ok(())
}

#[test]
fn file_database_uses_wal_and_survives_reopen() -> TestResult {
    let dir = temp_dir("reopen");
    let path = dir.join("teste.db");

    {
        let db = Database::open(&path)?;
        let mode: String = db
            .connection()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!(mode, "wal");
    }

    // Reabrir não deve reaplicar migrations nem falhar.
    let reopened = Database::open(&path)?;
    assert_eq!(reopened.schema_version()?, CURRENT_VERSION);
    drop(reopened);

    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn applying_migrations_twice_is_harmless() -> TestResult {
    let mut db = Database::open_in_memory()?;
    migrations::apply(db.connection_mut())?;
    assert_eq!(db.schema_version()?, CURRENT_VERSION);
    Ok(())
}

#[test]
fn newer_schema_version_is_rejected() -> TestResult {
    let mut db = Database::open_in_memory()?;
    db.connection_mut().pragma_update(None, "user_version", 99)?;

    let result = migrations::apply(db.connection_mut());
    assert!(matches!(result, Err(StorageError::SchemaTooNew { found: 99, .. })));
    Ok(())
}

#[test]
fn check_constraint_rejects_empty_action() -> TestResult {
    let db = Database::open_in_memory()?;
    let result = db.connection().execute(
        "INSERT INTO audit_log (occurred_at, action, entity) VALUES ('2026-01-01T00:00:00', '', 'produto')",
        [],
    );
    assert!(result.is_err());
    Ok(())
}

#[test]
fn uncommitted_transaction_is_rolled_back() -> TestResult {
    let mut db = Database::open_in_memory()?;
    {
        let tx = db.connection_mut().transaction()?;
        insert_audit(&tx)?;
        // Sem commit: ao sair do escopo, a transação é desfeita.
    }
    assert_eq!(count_audit(&db)?, 0);
    Ok(())
}

#[test]
fn committed_transaction_persists() -> TestResult {
    let mut db = Database::open_in_memory()?;
    let tx = db.connection_mut().transaction()?;
    insert_audit(&tx)?;
    tx.commit()?;
    assert_eq!(count_audit(&db)?, 1);
    Ok(())
}