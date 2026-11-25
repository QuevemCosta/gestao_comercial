use super::errors::DomainError;

/// Remove espaços das extremidades e rejeita textos vazios.
pub fn required_text(field: &'static str, raw: &str) -> Result<String, DomainError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Err(DomainError::EmptyField(field))
    } else {
        Ok(trimmed.to_owned())
    }
}