use std::path::Path;

use thiserror::Error;
use tracing::Level;

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("não foi possível criar a pasta de logs: {0}")]
    Io(#[from] std::io::Error),

    #[error("o sistema de logs já foi inicializado")]
    AlreadyInitialized,
}

/// Inicializa os logs em arquivo, com rotação diária.
///
/// O `WorkerGuard` retornado deve permanecer vivo até o fim do programa:
/// ao ser descartado, as mensagens pendentes são gravadas.
pub fn init(log_dir: &Path) -> Result<tracing_appender::non_blocking::WorkerGuard, LoggingError> {
    std::fs::create_dir_all(log_dir)?;

    let appender = tracing_appender::rolling::daily(log_dir, "gestao_comercial.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(Level::INFO)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInitialized)?;

    Ok(guard)
}