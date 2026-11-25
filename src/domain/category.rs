use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::validation::required_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    id: Uuid,
    name: String,
    active: bool,
    created_at: NaiveDateTime,
}

impl Category {
    pub fn create(id: Uuid, name: &str, created_at: NaiveDateTime) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            name: required_text("nome da categoria", name)?,
            active: true,
            created_at,
        })
    }

    /// Reconstrói a entidade a partir do armazenamento, sem revalidar.
    pub fn restore(id: Uuid, name: String, active: bool, created_at: NaiveDateTime) -> Self {
        Self { id, name, active, created_at }
    }

    pub fn rename(&mut self, name: &str) -> Result<(), DomainError> {
        self.name = required_text("nome da categoria", name)?;
        Ok(())
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn created_at(&self) -> NaiveDateTime {
        self.created_at
    }
}