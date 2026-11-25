use thiserror::Error;

use crate::domain::errors::{DomainError, RepositoryError};

/// Erros expostos à interface. Não contêm detalhes do SQLite.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppError {
    #[error("{0}")]
    Validation(DomainError),
    #[error("{0} não encontrado")]
    NotFound(&'static str),
    #[error("{0}")]
    Conflict(String),
    #[error("falha interna: {0}")]
    Storage(String),
}

impl From<DomainError> for AppError {
    fn from(err: DomainError) -> Self {
        AppError::Validation(err)
    }
}

impl From<RepositoryError> for AppError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::NotFound => AppError::NotFound("registro"),
            RepositoryError::Conflict(message) => AppError::Conflict(message),
            RepositoryError::Rule(rule) => AppError::Validation(rule),
            RepositoryError::Storage(message) => AppError::Storage(message),
        }
    }
}