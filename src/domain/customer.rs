use chrono::NaiveDateTime;
use uuid::Uuid;

use super::contact::normalize_phone;
use super::document::{TaxId, TaxIdKind};
use super::errors::DomainError;
use super::validation::required_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonType {
    Individual,
    Company,
}

impl PersonType {
    pub fn as_str(self) -> &'static str {
        match self {
            PersonType::Individual => "individual",
            PersonType::Company => "company",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "individual" => Some(PersonType::Individual),
            "company" => Some(PersonType::Company),
            _ => None,
        }
    }

    fn expected_document(self) -> TaxIdKind {
        match self {
            PersonType::Individual => TaxIdKind::Cpf,
            PersonType::Company => TaxIdKind::Cnpj,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    street: String,
    neighborhood: String,
    city: String,
}

impl Address {
    pub fn new(street: &str, neighborhood: &str, city: &str) -> Result<Self, DomainError> {
        Ok(Self {
            street: required_text("logradouro", street)?,
            neighborhood: required_text("bairro", neighborhood)?,
            city: required_text("cidade", city)?,
        })
    }

    /// Reconstrói o endereço a partir do armazenamento, sem revalidar.
    pub fn restore(street: String, neighborhood: String, city: String) -> Self {
        Self { street, neighborhood, city }
    }

    pub fn street(&self) -> &str {
        &self.street
    }

    pub fn neighborhood(&self) -> &str {
        &self.neighborhood
    }

    pub fn city(&self) -> &str {
        &self.city
    }
}

/// Dados editáveis de um cliente. Após `validated`, documento e telefones contêm somente dígitos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerData {
    pub person_type: PersonType,
    pub name: String,
    pub document: Option<String>,
    pub phone: Option<String>,
    pub whatsapp: Option<String>,
    pub address: Option<Address>,
    pub notes: String,
}

impl CustomerData {
    pub fn validated(self) -> Result<Self, DomainError> {
        let document = match non_empty(self.document.as_deref()) {
            Some(raw) => {
                let tax_id = TaxId::parse(raw)?;
                if tax_id.kind() != self.person_type.expected_document() {
                    return Err(DomainError::TaxIdKindMismatch);
                }
                Some(tax_id.digits().to_owned())
            }
            None => None,
        };

        let phone = normalize_optional_phone(self.phone.as_deref())?;
        let whatsapp = normalize_optional_phone(self.whatsapp.as_deref())?;
        if phone.is_none() && whatsapp.is_none() {
            return Err(DomainError::ContactRequired);
        }

        Ok(Self {
            person_type: self.person_type,
            name: required_text("nome", &self.name)?,
            document,
            phone,
            whatsapp,
            address: self.address,
            notes: self.notes.trim().to_owned(),
        })
    }
}

fn non_empty(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

fn normalize_optional_phone(raw: Option<&str>) -> Result<Option<String>, DomainError> {
    non_empty(raw).map(normalize_phone).transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customer {
    id: Uuid,
    data: CustomerData,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl Customer {
    pub fn create(id: Uuid, data: CustomerData, now: NaiveDateTime) -> Result<Self, DomainError> {
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
        data: CustomerData,
        active: bool,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self { id, data, active, created_at, updated_at }
    }

    pub fn update(&mut self, data: CustomerData, now: NaiveDateTime) -> Result<(), DomainError> {
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

    pub fn data(&self) -> &CustomerData {
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
    use chrono::NaiveDate;

    use super::*;

    fn base(person_type: PersonType) -> CustomerData {
        CustomerData {
            person_type,
            name: "Ana Paula".to_owned(),
            document: None,
            phone: Some("(11) 91234-5678".to_owned()),
            whatsapp: None,
            address: None,
            notes: "  obs  ".to_owned(),
        }
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("data de teste válida")
    }

    #[test]
    fn phone_and_notes_are_normalized() -> Result<(), DomainError> {
        let data = base(PersonType::Individual).validated()?;
        assert_eq!(data.phone.as_deref(), Some("11912345678"));
        assert_eq!(data.notes, "obs");
        Ok(())
    }

    #[test]
    fn contact_is_required() {
        let data = CustomerData { phone: None, ..base(PersonType::Individual) };
        assert_eq!(data.validated(), Err(DomainError::ContactRequired));
    }

    #[test]
    fn document_must_match_person_type() {
        let data = CustomerData {
            document: Some("11222333000181".to_owned()),
            ..base(PersonType::Individual)
        };
        assert_eq!(data.validated(), Err(DomainError::TaxIdKindMismatch));
    }

    #[test]
    fn new_customer_is_active() -> Result<(), DomainError> {
        let customer = Customer::create(Uuid::now_v7(), base(PersonType::Individual), now())?;
        assert!(customer.is_active());
        Ok(())
    }
}