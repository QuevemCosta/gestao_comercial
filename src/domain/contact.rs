use super::errors::DomainError;

/// Normaliza telefone brasileiro para DDD + número (10 ou 11 dígitos).
/// Aceita o prefixo do país 55 quando presente.
pub fn normalize_phone(raw: &str) -> Result<String, DomainError> {
    let allowed = raw
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '-' | '(' | ')' | '+'));
    if !allowed {
        return Err(DomainError::InvalidPhone);
    }

    let mut digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if (digits.len() == 12 || digits.len() == 13) && digits.starts_with("55") {
        digits.drain(..2);
    }

    match digits.len() {
        10 | 11 => Ok(digits),
        _ => Err(DomainError::InvalidPhone),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_normalized_to_digits() -> Result<(), DomainError> {
        assert_eq!(normalize_phone("(11) 91234-5678")?, "11912345678");
        Ok(())
    }

    #[test]
    fn country_prefix_is_removed() -> Result<(), DomainError> {
        assert_eq!(normalize_phone("+55 85 3456-7890")?, "8534567890");
        Ok(())
    }

    #[test]
    fn short_numbers_are_rejected() {
        assert_eq!(normalize_phone("123"), Err(DomainError::InvalidPhone));
    }
}