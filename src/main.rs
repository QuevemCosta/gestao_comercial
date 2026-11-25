#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Context;
use chrono::Local;

use gestao_comercial::infrastructure::backup;
use gestao_comercial::infrastructure::config::{self, AppConfig};
use gestao_comercial::infrastructure::database::Database;
use gestao_comercial::infrastructure::logging;
use gestao_comercial::presentation;

fn main() -> anyhow::Result<()> {
    let config = AppConfig::local_default();

    // O guard precisa viver até o fim do programa para que os logs sejam gravados.
    let _log_guard = logging::init(&config.log_dir).context("falha ao iniciar o sistema de logs")?;
    std::panic::set_hook(Box::new(|info| {
        tracing::error!(panic = %info, "pânico na aplicação");
    }));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "aplicação iniciada");
    tracing::info!(data = %config.database_path.display(), "pasta de dados em uso");

    let result = run(&config);
    if let Err(error) = &result {
        tracing::error!(error = %format!("{error:#}"), "encerrado por erro");
    }
    result
}

fn run(config: &AppConfig) -> anyhow::Result<()> {
    match config::migrate_legacy_database(&config.database_path) {
        Ok(true) => tracing::info!("banco copiado da pasta antiga para a pasta de dados atual"),
        Ok(false) => {}
        Err(error) => tracing::error!(%error, "falha ao copiar o banco da pasta antiga"),
    }

    apply_pending_restore(config);

    let db = Database::open(&config.database_path)
        .with_context(|| format!("falha ao abrir o banco em {}", config.database_path.display()))?;
    let version = db.schema_version()?;
    tracing::info!(schema_version = version, "banco de dados pronto");

    run_automatic_backup(&db, config);

    presentation::run(db).map_err(|error| anyhow::anyhow!("falha na interface gráfica: {error}"))?;
    Ok(())
}

/// Aplica uma restauração preparada pela tela de configurações. Se falhar, o banco atual é mantido.
fn apply_pending_restore(config: &AppConfig) {
    let now = Local::now().naive_local();
    match backup::apply_pending_restore(&config.database_path, now) {
        Ok(Some(previous)) => {
            tracing::info!(previous = %previous.display(), "backup restaurado; banco anterior preservado");
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(%error, "restauração pendente não aplicada; o banco atual será mantido");
        }
    }
}

/// Backup diário automático. Falha é registrada no log e não impede a abertura do sistema.
fn run_automatic_backup(db: &Database, config: &AppConfig) {
    let now = Local::now().naive_local();
    let dir = config.backup_dir();
    match backup::ensure_daily_backup(db.connection(), &dir, now) {
        Ok(Some(path)) => tracing::info!(backup = %path.display(), "backup automático criado e verificado"),
        Ok(None) => tracing::debug!("backup automático de hoje já existe"),
        Err(error) => tracing::error!(%error, "falha no backup automático; o sistema continuará normalmente"),
    }
}