use super::api::SymbolKind;
use regex::Regex;

pub fn definition_regexes(symbol: &str) -> Result<Vec<(SymbolKind, Regex)>, regex::Error> {
    let symbol = regex::escape(symbol);
    Ok(vec![
        (
            SymbolKind::Function,
            Regex::new(&format!(r"\b(fn|function|def)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Class,
            Regex::new(&format!(r"\b(class)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Struct,
            Regex::new(&format!(r"\b(struct)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Enum,
            Regex::new(&format!(r"\b(enum)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Trait,
            Regex::new(&format!(r"\b(trait)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Interface,
            Regex::new(&format!(r"\b(interface)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Type,
            Regex::new(&format!(r"\b(type)\s+{}\b", symbol))?,
        ),
        (
            SymbolKind::Variable,
            Regex::new(&format!(r"\b(let|const|var)\s+{}\b", symbol))?,
        ),
    ])
}
