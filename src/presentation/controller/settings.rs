//! Configurações: pastas do sistema, backup e restauração.

use std::path::{Path, PathBuf};

use slint::SharedString;

use crate::infrastructure::backup;
use crate::infrastructure::config::AppConfig;

use super::{msg, Controller};

/// Caminhos de armazenamento. Derivados da configuração local; a Etapa 9 os move
/// para a pasta de dados do usuário.
pub struct StoragePaths {
    pub database: PathBuf,
    pub backups: PathBuf,
    pub exports: PathBuf,
}

pub fn storage_paths() -> StoragePaths {
    let config = AppConfig::local_default();
    let base = config
        .database_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    StoragePaths {
        backups: base.join("backups"),
        exports: base.join("exportacoes"),
        database: config.database_path,
    }
}

pub struct SettingsView {
    pub data_folder: String,
    pub database_file: String,
    pub backups_folder: String,
    pub exports_folder: String,
    pub schema_version: String,
    pub backup_options: Vec<SharedString>,
}

impl Controller {
    pub fn settings_view(&mut self) -> Result<SettingsView, String> {
        let paths = storage_paths();
        let backups = backup::list_backups(&paths.backups).map_err(msg)?;
        let data_folder = paths
            .database
            .parent()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();

        Ok(SettingsView {
            data_folder,
            database_file: paths.database.display().to_string(),
            backups_folder: paths.backups.display().to_string(),
            exports_folder: paths.exports.display().to_string(),
            schema_version: self.db.schema_version().map_err(msg)?.to_string(),
            backup_options: backups
                .iter()
                .map(|path| SharedString::from(file_name(path)))
                .collect(),
        })
    }

    pub fn create_backup(&mut self) -> Result<String, String> {
        let paths = storage_paths();
        let now = self.now();
        let path = backup::create_backup(self.db.connection(), &paths.backups, now).map_err(msg)?;
        Ok(format!("Backup criado: {}", path.display()))
    }

    /// Prepara a restauração do backup escolhido. Só é aplicada após reiniciar o sistema.
    pub fn prepare_restore(&mut self, backup_index: i32, confirmed: bool) -> Result<String, String> {
        if !confirmed {
            return Err("marque a confirmação antes de restaurar um backup".to_owned());
        }
        let paths = storage_paths();
        let backups = backup::list_backups(&paths.backups).map_err(msg)?;
        let chosen = usize::try_from(backup_index)
            .ok()
            .and_then(|position| backups.get(position))
            .ok_or_else(|| "selecione o backup a restaurar".to_owned())?;

        backup::stage_restore(chosen, &paths.database).map_err(msg)?;
        Ok(
            "Restauração preparada. Feche e abra o sistema para concluí-la; \
             o banco atual será guardado com o sufixo .antes-restauracao."
                .to_owned(),
        )
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}