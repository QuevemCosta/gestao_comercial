//! Backup e restauração do banco local.
//!
//! - O backup usa `VACUUM INTO`, que gera uma cópia consistente mesmo com o sistema aberto.
//! - Backups automáticos e manuais usam prefixos diferentes. A retenção só apaga automáticos.
//! - A restauração é preparada em um arquivo `.restaurar` e aplicada na inicialização,
//!   antes de o banco ser aberto, para nunca substituir um arquivo em uso.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OpenFlags};

use super::error::StorageError;
use super::migrations::CURRENT_VERSION;

const MANUAL_PREFIX: &str = "backup-";
const AUTO_PREFIX: &str = "auto-";
const BACKUP_EXTENSION: &str = "db";
const TIMESTAMP_FORMAT: &str = "%Y%m%d-%H%M%S";
const DAY_FORMAT: &str = "%Y%m%d";
const RESTORE_SUFFIX: &str = ".restaurar";
const KEPT_SUFFIX: &str = ".antes-restauracao";

/// Quantidade de backups automáticos mantidos.
pub const AUTO_RETENTION: usize = 30;

/// Cria um backup manual consistente em `dir`. O nome contém a data e a hora.
pub fn create_backup(conn: &Connection, dir: &Path, now: NaiveDateTime) -> Result<PathBuf, StorageError> {
    fs::create_dir_all(dir)?;
    let target = dir.join(format!(
        "{MANUAL_PREFIX}{}.{BACKUP_EXTENSION}",
        now.format(TIMESTAMP_FORMAT)
    ));
    write_snapshot(conn, &target)?;
    Ok(target)
}

/// Garante um backup automático por dia.
///
/// Se já existir um automático do dia, não faz nada. Caso contrário, cria o backup,
/// verifica a integridade e aplica a retenção. Se a verificação falhar, o arquivo
/// novo é removido e os anteriores permanecem intactos.
pub fn ensure_daily_backup(conn: &Connection, dir: &Path, now: NaiveDateTime) -> Result<Option<PathBuf>, StorageError> {
    fs::create_dir_all(dir)?;
    let today = now.format(DAY_FORMAT).to_string();
    if automatic_backups(dir)?
        .iter()
        .any(|path| file_day(path).as_deref() == Some(today.as_str()))
    {
        return Ok(None);
    }

    let target = dir.join(format!(
        "{AUTO_PREFIX}{}.{BACKUP_EXTENSION}",
        now.format(TIMESTAMP_FORMAT)
    ));
    write_snapshot(conn, &target)?;

    if let Err(error) = validate_backup(&target) {
        let _ = fs::remove_file(&target);
        return Err(error);
    }

    apply_retention(dir)?;
    Ok(Some(target))
}

/// Backups existentes, do mais recente para o mais antigo. Inclui manuais e automáticos.
pub fn list_backups(dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_backup_file(path))
        .collect();
    // Os nomes contêm data e hora, então a ordem lexical é a cronológica.
    files.sort_by(|a, b| b.cmp(a));
    Ok(files)
}

/// Verifica integridade e versão do esquema. Devolve a versão encontrada.
pub fn validate_backup(path: &Path) -> Result<i64, StorageError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let check: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("backup inválido: {check}"),
        )
        .into());
    }
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > CURRENT_VERSION {
        return Err(StorageError::SchemaTooNew {
            found: version,
            supported: CURRENT_VERSION,
        });
    }
    Ok(version)
}

/// Caminho do arquivo que aguarda restauração na próxima inicialização.
pub fn pending_restore_path(database: &Path) -> PathBuf {
    sidecar(database, RESTORE_SUFFIX)
}

/// Valida o backup e o deixa pronto para ser aplicado na próxima inicialização.
pub fn stage_restore(backup: &Path, database: &Path) -> Result<(), StorageError> {
    validate_backup(backup)?;
    fs::copy(backup, pending_restore_path(database))?;
    Ok(())
}

/// Aplica a restauração pendente, se existir. Devolve o caminho onde o banco anterior foi guardado.
pub fn apply_pending_restore(database: &Path, now: NaiveDateTime) -> Result<Option<PathBuf>, StorageError> {
    let pending = pending_restore_path(database);
    if !pending.exists() {
        return Ok(None);
    }
    validate_backup(&pending)?;

    let stamp = now.format(TIMESTAMP_FORMAT).to_string();
    let kept = if database.exists() {
        let kept = renamed_database(database, &format!("{KEPT_SUFFIX}-{stamp}"));
        fs::rename(database, &kept)?;
        for side in ["-wal", "-shm"] {
            let source = sidecar(database, side);
            if source.exists() {
                fs::rename(&source, sidecar(&kept, side))?;
            }
        }
        Some(kept)
    } else {
        None
    };

    fs::rename(&pending, database)?;
    Ok(kept)
}

// ---------- Funções auxiliares ----------

/// Gera o snapshot em `target`. Recusa sobrescrever um arquivo existente.
fn write_snapshot(conn: &Connection, target: &Path) -> Result<(), StorageError> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "já existe um backup criado neste mesmo segundo",
        )
        .into());
    }
    conn.execute(
        "VACUUM INTO ?1",
        params![target.to_string_lossy().into_owned()],
    )?;
    Ok(())
}

/// Backups automáticos, do mais recente para o mais antigo.
fn automatic_backups(dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
    Ok(list_backups(dir)?
        .into_iter()
        .filter(|path| file_name(path).starts_with(AUTO_PREFIX))
        .collect())
}

/// Mantém os `AUTO_RETENTION` automáticos mais recentes. Nunca toca em backups manuais.
fn apply_retention(dir: &Path) -> Result<(), StorageError> {
    let autos = automatic_backups(dir)?;
    for old in autos.into_iter().skip(AUTO_RETENTION) {
        fs::remove_file(&old)?;
    }
    Ok(())
}

fn is_backup_file(path: &Path) -> bool {
    let name = file_name(path);
    (name.starts_with(MANUAL_PREFIX) || name.starts_with(AUTO_PREFIX))
        && name.ends_with(&format!(".{BACKUP_EXTENSION}"))
}

/// Data (AAAAMMDD) extraída do nome do arquivo automático.
fn file_day(path: &Path) -> Option<String> {
    let name = file_name(path);
    let stamp = name
        .strip_prefix(AUTO_PREFIX)?
        .strip_suffix(&format!(".{BACKUP_EXTENSION}"))?;
    let day = stamp.split('-').next()?;
    NaiveDate::parse_from_str(day, DAY_FORMAT)
        .ok()
        .map(|date| date.format(DAY_FORMAT).to_string())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Nome do banco com um sufixo antes da extensão: `gestao.db` → `gestao.<sufixo>.db`.
fn renamed_database(database: &Path, suffix: &str) -> PathBuf {
    let stem = database
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("gestao_comercial");
    let extension = database
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or(BACKUP_EXTENSION);
    database.with_file_name(format!("{stem}{suffix}.{extension}"))
}

/// Arquivo acessório do SQLite: o nome completo do banco seguido do sufixo (`-wal`, `-shm`, `.restaurar`).
fn sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut name = database
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    database.with_file_name(name)
}