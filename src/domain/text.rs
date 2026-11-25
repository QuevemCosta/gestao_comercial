use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Chave de comparação para buscas: minúsculas e sem acentos.
/// "Cadeira Giratória" e "cadeira giratoria" produzem a mesma chave.
pub fn search_key(text: &str) -> String {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_accents_and_lowercases() {
        assert_eq!(search_key("Cadeira Giratória"), "cadeira giratoria");
        assert_eq!(search_key("MESA DE AÇO"), "mesa de aco");
    }

    #[test]
    fn leaves_plain_text_unchanged_apart_from_case() {
        assert_eq!(search_key("SKU-01"), "sku-01");
    }
}