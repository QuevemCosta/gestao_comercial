use chrono::NaiveDateTime;
use uuid::Uuid;

use super::contact::normalize_phone;
use super::document::{TaxId, TaxIdKind};
use super::errors::DomainError;
use super::validation::required_text;

/// Dados editáveis de um entregador. O CPF é opcional; quando informado, é validado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelivererData {
    pub name: String,
    pub phone: String,
    pub document: Option<String>,
    pub notes: String,
}

impl DelivererData {
    pub fn validated(self) -> Result<Self, DomainError> {
        let document = match self.document.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(raw) => {
                let tax_id = TaxId::parse(raw)?;
                if tax_id.kind() != TaxIdKind::Cpf {
                    return Err(DomainError::DelivererDocumentMustBeCpf);
                }
                Some(tax_id.digits().to_owned())
            }
            None => None,
        };

        Ok(Self {
            name: required_text("nome", &self.name)?,
            phone: normalize_phone(&self.phone)?,
            document,
            notes: self.notes.trim().to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deliverer {
    id: Uuid,
    data: DelivererData,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl Deliverer {
    pub fn create(id: Uuid, data: DelivererData, now: NaiveDateTime) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            data: data.validated()?,
            active: true,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn restore(
        id: Uuid,
        data: DelivererData,
        active: bool,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self { id, data, active, created_at, updated_at }
    }

    pub fn update(&mut self, data: DelivererData, now: NaiveDateTime) -> Result<(), DomainError> {
        self.data = data.validated()?;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_active(&mut self, active: bool, now: NaiveDateTime) {
        self.active = active;
        self.updated_at = now;
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn data(&self) -> &DelivererData {
        &self.data
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn created_at(&self) -> NaiveDateTime {
        self.created_at
    }

    pub fn updated_at(&self) -> NaiveDateTime {
        self.updated_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliverer_document_must_be_cpf() {
        let data = DelivererData {
            name: "Carlos".to_owned(),
            phone: "85988887777".to_owned(),
            document: Some("11.222.333/0001-81".to_owned()),
            notes: String::new(),
        };
        assert_eq!(data.validated(), Err(DomainError::DelivererDocumentMustBeCpf));
    }

    #[test]
    fn document_is_optional() -> Result<(), DomainError> {
        let data = DelivererData {
            name: "Carlos".to_owned(),
            phone: "85988887777".to_owned(),
            document: Some("  ".to_owned()),
            notes: String::new(),
        };
        assert_eq!(data.validated()?.document, None);
        Ok(())
    }
}