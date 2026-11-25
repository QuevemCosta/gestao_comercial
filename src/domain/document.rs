use super::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxIdKind {
    Cpf,
    Cnpj,
}

/// CPF ou CNPJ com dígitos verificadores validados. Armazenado somente com dígitos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxId {
    digits: String,
    kind: TaxIdKind,
}

impl TaxId {
    /// Aceita documento com ou sem pontuação (`.`, `-`, `/`, espaço). Letras são rejeitadas.
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let allowed = raw
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '/' | ' '));
        if !allowed {
            return Err(DomainError::InvalidTaxId);
        }

        let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
        match digits.len() {
            11 if is_valid_cpf(&digits) => Ok(Self { digits, kind: TaxIdKind::Cpf }),
            14 if is_valid_cnpj(&digits) => Ok(Self { digits, kind: TaxIdKind::Cnpj }),
            _ => Err(DomainError::InvalidTaxId),
        }
    }

    pub fn digits(&self) -> &str {
        &self.digits
    }

    pub fn kind(&self) -> TaxIdKind {
        self.kind
    }
}

fn to_digit_values(digits: &str) -> Vec<u32> {
    digits.bytes().map(|b| u32::from(b - b'0')).collect()
}

fn is_valid_cpf(digits: &str) -> bool {
    let values = to_digit_values(digits);
    if values.len() != 11 || values.iter().all(|value| *value == values[0]) {
        return false;
    }
    cpf_check_digit(&values[..9], 10) == values[9]
        && cpf_check_digit(&values[..10], 11) == values[10]
}

fn cpf_check_digit(values: &[u32], start_weight: u32) -> u32 {
    let sum: u32 = values
        .iter()
        .enumerate()
        .map(|(index, value)| value * (start_weight - index as u32))
        .sum();
    let rest = (sum * 10) % 11;
    if rest == 10 {
        0
    } else {
        rest
    }
}

fn is_valid_cnpj(digits: &str) -> bool {
    const WEIGHTS_FIRST: [u32; 12] = [5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    const WEIGHTS_SECOND: [u32; 13] = [6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];

    let values = to_digit_values(digits);
    if values.len() != 14 || values.iter().all(|value| *value == values[0]) {
        return false;
    }
    cnpj_check_digit(&values[..12], &WEIGHTS_FIRST) == values[12]
        && cnpj_check_digit(&values[..13], &WEIGHTS_SECOND) == values[13]
}

fn cnpj_check_digit(values: &[u32], weights: &[u32]) -> u32 {
    let sum: u32 = values.iter().zip(weights).map(|(value, weight)| value * weight).sum();
    let rest = sum % 11;
    if rest < 2 {
        0
    } else {
        11 - rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_cpf_with_punctuation() -> Result<(), DomainError> {
        let tax_id = TaxId::parse("529.982.247-25")?;
        assert_eq!(tax_id.kind(), TaxIdKind::Cpf);
        assert_eq!(tax_id.digits(), "52998224725");
        Ok(())
    }

    #[test]
    fn rejects_cpf_with_wrong_check_digit() {
        assert_eq!(TaxId::parse("52998224726"), Err(DomainError::InvalidTaxId));
    }

    #[test]
    fn rejects_cpf_with_repeated_digits() {
        assert_eq!(TaxId::parse("11111111111"), Err(DomainError::InvalidTaxId));
    }

    #[test]
    fn accepts_valid_cnpj() -> Result<(), DomainError> {
        let tax_id = TaxId::parse("11.222.333/0001-81")?;
        assert_eq!(tax_id.kind(), TaxIdKind::Cnpj);
        Ok(())
    }

    #[test]
    fn rejects_cnpj_with_wrong_check_digit() {
        assert_eq!(TaxId::parse("11222333000182"), Err(DomainError::InvalidTaxId));
    }

    #[test]
    fn rejects_letters() {
        assert_eq!(TaxId::parse("529.982.247-2A"), Err(DomainError::InvalidTaxId));
    }
}