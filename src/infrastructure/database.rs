use std::path::Path;
use std::time::Duration;

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::application::error::AppError;
use crate::application::unit_of_work::{Transactor, UnitOfWork};
use crate::domain::text::search_key;

use super::error::StorageError;
use super::migrations;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Conexão com o banco SQLite local. Único ponto que abre conexões.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Abre (ou cria) um banco em arquivo, usando WAL para leituras durante escritas.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::finish_setup(conn)
    }

    /// Banco em memória, usado nos testes.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::finish_setup(Connection::open_in_memory()?)
    }

    fn finish_setup(mut conn: Connection) -> Result<Self, StorageError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(BUSY_TIMEOUT)?;

        let foreign_keys: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        if foreign_keys != 1 {
            return Err(StorageError::ForeignKeysDisabled);
        }

        register_functions(&conn)?;
        migrations::apply(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn schema_version(&self) -> Result<i64, StorageError> {
        migrations::current_version(&self.conn)
    }
}

/// Funções SQL próprias da aplicação.
/// `search_key(texto)` permite buscas sem diferenciar acentos nem maiúsculas.
fn register_functions(conn: &Connection) -> Result<(), StorageError> {
    conn.create_scalar_function(
        "search_key",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let text: Option<String> = ctx.get(0)?;
            Ok(text.map(|value| search_key(&value)))
        },
    )?;
    Ok(())
}

/// Unidade de trabalho sobre uma transação SQLite com bloqueio de escrita imediato.
/// Ao ser descartada sem `commit`, a transação é desfeita automaticamente.
pub struct SqliteUnitOfWork<'a> {
    tx: Transaction<'a>,
}

impl SqliteUnitOfWork<'_> {
    pub(crate) fn conn(&self) -> &Connection {
                &self.tx
    }
}

impl UnitOfWork for SqliteUnitOfWork<'_> {
    fn commit(self: Box<Self>) -> Result<(), AppError> {
        let unit = *self;
        unit.tx.commit().map_err(to_app_error)
    }
}

impl Transactor for Database {
    fn begin(&mut self) -> Result<Box<dyn UnitOfWork + '_>, AppError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(to_app_error)?;
        let unit: Box<dyn UnitOfWork + '_> = Box::new(SqliteUnitOfWork { tx });
        Ok(unit)
    }
}

fn to_app_error(err: rusqlite::Error) -> AppError {
    AppError::Storage(err.to_string())
}