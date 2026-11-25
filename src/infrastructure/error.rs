use thiserror::Error;

/// Erros da camada de persistência. A camada de aplicação os converterá
/// em erros de caso de uso, sem expor detalhes do SQLite à interface.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("erro de acesso ao sistema de arquivos: {0}")]
    Io(#[from] std::io::Error),

    #[error("erro no banco de dados: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("falha ao aplicar a migração {version}: {source}")]
    Migration {
        version: i64,
        source: rusqlite::Error,
    },

    #[error(
        "o banco está na versão {found}, mas esta aplicação suporta até a versão {supported}"
    )]
    SchemaTooNew { found: i64, supported: i64 },

    #[error("o SQLite não confirmou a ativação das chaves estrangeiras")]
    ForeignKeysDisabled,
}