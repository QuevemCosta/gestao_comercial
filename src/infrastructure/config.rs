use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// Caminhos de execução.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub database_path: PathBuf,
    pub log_dir: PathBuf,
}

impl AppConfig {
    /// Configuração padrão: banco, logs e backups ficam na pasta de dados escolhida por `data_dir`.
    pub fn local_default() -> Self {
        let base = data_dir();
        Self {
            database_path: base.join("gestao_comercial.db"),
            log_dir: base.join("logs"),
        }
    }

        /// Pasta de backups, ao lado do banco.
    pub fn backup_dir(&self) -> PathBuf {
        self.database_path
            .parent()
            .map(|dir| dir.join("backups"))
            .unwrap_or_else(|| PathBuf::from("backups"))
    }
}

/// Pasta de dados. Prioridade:
/// 1. `Dados` ao lado do executável (ex.: `C:\GestaoComercial\Dados`), se for gravável.
/// 2. Pasta local do usuário, se a primeira não puder ser usada.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = install_data_dir() {
        if is_writable(&dir) {
            return dir;
        }
    }
    fallback_data_dir()
}

/// Pasta de dados antiga, usada até a versão 1.0.0 do instalador.
pub fn legacy_data_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "GestaoComercial").map(|dirs| dirs.data_local_dir().to_path_buf())
}

/// Copia o banco da pasta antiga para a nova, uma única vez, quando a nova ainda não existe.
/// Devolve `true` se a cópia foi feita. A pasta antiga não é alterada.
pub fn migrate_legacy_database(current: &Path) -> std::io::Result<bool> {
    if current.exists() {
        return Ok(false);
    }
    let Some(legacy_dir) = legacy_data_dir() else {
        return Ok(false);
    };
    let legacy = legacy_dir.join("gestao_comercial.db");
    if !legacy.exists() || legacy == current {
        return Ok(false);
    }
    if let Some(parent) = current.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&legacy, current)?;
    // Arquivos auxiliares do modo WAL, se existirem, acompanham o banco.
    for suffix in ["-wal", "-shm"] {
        let source = legacy_dir.join(format!("gestao_comercial.db{suffix}"));
        if source.exists() {
            fs::copy(&source, current.with_file_name(format!("gestao_comercial.db{suffix}")))?;
        }
    }
    Ok(true)
}

fn install_data_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("Dados"))
}

fn fallback_data_dir() -> PathBuf {
    legacy_data_dir().unwrap_or_else(|| PathBuf::from("dados"))
}

/// Testa gravação de fato, criando e apagando um arquivo de teste.
fn is_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".teste-escrita");
    match fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}