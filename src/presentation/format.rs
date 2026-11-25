use crate::domain::value_objects::Money;

/// Converte um valor digitado (`12,50`, `R$ 1.234,56`, `7`, `1.50`) em centavos.
pub fn parse_money(field: &str, raw: &str) -> Result<Money, String> {
    let invalid = || format!("valor inválido em '{field}'");

    let cleaned: String = raw
        .replace("R$", "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if cleaned.is_empty() {
        return Err(format!("informe o campo '{field}'"));
    }

    // Com vírgula decimal, o ponto é separador de milhar; sem vírgula, o ponto é decimal.
    let normalized = if cleaned.contains(',') {
        cleaned.replace('.', "").replacen(',', ".", 1)
    } else {
        cleaned
    };

    let (reais_text, cents_text) = normalized
        .split_once('.')
        .unwrap_or((normalized.as_str(), ""));
    let reais_text = if reais_text.is_empty() { "0" } else { reais_text };

    let only_digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    if !only_digits(reais_text) || !only_digits(cents_text) || cents_text.len() > 2 {
        return Err(invalid());
    }

    let reais: i64 = reais_text.parse().map_err(|_| invalid())?;
    let cents: i64 = if cents_text.is_empty() {
        0
    } else {
        // "5" vira "50": completa à direita com zeros.
        format!("{cents_text:0<2}").parse().map_err(|_| invalid())?
    };

    reais
        .checked_mul(100)
        .and_then(|value| value.checked_add(cents))
        .map(Money::from_cents)
        .ok_or_else(invalid)
}

/// Valor sem símbolo e sem separador de milhar, para preencher campos de edição: `1234,56`.
pub fn plain_money(value: Money) -> String {
    let cents = value.cents();
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!("{sign}{},{:02}", absolute / 100, absolute % 100)
}

pub fn parse_int(field: &str, raw: &str) -> Result<i64, String> {
    raw.trim()
        .parse::<i64>()
        .map_err(|_| format!("informe um número inteiro em '{field}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_decimal_values() {
        assert_eq!(parse_money("preço", "12,50"), Ok(Money::from_cents(1250)));
        assert_eq!(parse_money("preço", "0,5"), Ok(Money::from_cents(50)));
        assert_eq!(parse_money("preço", "7"), Ok(Money::from_cents(700)));
        assert_eq!(parse_money("preço", "1.50"), Ok(Money::from_cents(150)));
    }

    #[test]
    fn accepts_currency_prefix_and_thousands_separator() {
        assert_eq!(
            parse_money("preço", "R$ 1.234,56"),
            Ok(Money::from_cents(123_456))
        );
    }

    #[test]
    fn rejects_invalid_money() {
        assert!(parse_money("preço", "").is_err());
        assert!(parse_money("preço", "abc").is_err());
        assert!(parse_money("preço", "1,234").is_err());
        assert!(parse_money("preço", "-5").is_err());
    }

    #[test]
    fn plain_money_has_no_symbol_or_grouping() {
        assert_eq!(plain_money(Money::from_cents(123_456)), "1234,56");
        assert_eq!(plain_money(Money::from_cents(5)), "0,05");
    }

    #[test]
    fn parse_int_rejects_text() {
        assert_eq!(parse_int("quantidade", " 12 "), Ok(12));
        assert!(parse_int("quantidade", "doze").is_err());
    }
}