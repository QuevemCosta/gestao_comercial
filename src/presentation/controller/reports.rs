//! Tela de relatórios: monta as tabelas, converte para a grade da interface e exporta CSV.

use std::fs;

use chrono::{Datelike, NaiveDate};
use slint::SharedString;

use crate::application::reports::{ReportKind, ReportTable, Reports};
use crate::presentation::ReportRow;

use super::settings::storage_paths;
use super::{msg, Controller};

/// Opções do seletor de relatórios, na ordem de `ReportKind::ALL`.
pub fn report_kind_options() -> Vec<SharedString> {
    ReportKind::ALL
        .iter()
        .map(|kind| SharedString::from(kind.label()))
        .collect()
}

/// Converte uma linha de texto na linha de cinco colunas usada pela grade.
pub fn report_row(cells: &[String]) -> ReportRow {
    let cell = |index: usize| SharedString::from(cells.get(index).map(String::as_str).unwrap_or(""));
    ReportRow {
        c1: cell(0),
        c2: cell(1),
        c3: cell(2),
        c4: cell(3),
        c5: cell(4),
    }
}

/// CSV com separador `;` (padrão do Excel em português) e BOM UTF-8 para acentos corretos.
pub fn to_csv(table: &ReportTable) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(&csv_line(&table.headers));
    out.push('\n');
    for row in &table.rows {
        out.push_str(&csv_line(row));
        out.push('\n');
    }
    out
}

fn csv_line(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| csv_cell(cell))
        .collect::<Vec<_>>()
        .join(";")
}

fn csv_cell(cell: &str) -> String {
    if cell.chars().any(|c| matches!(c, ';' | '"' | '\n')) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_owned()
    }
}

impl Controller {
    pub fn report_table(&mut self, kind_index: i32, from: &str, to: &str) -> Result<ReportTable, String> {
        let kind = ReportKind::from_index(kind_index);
        let (from_day, to_day) = self.report_period(kind, from, to)?;
        Reports::new(&mut self.db, &self.clock)
            .build(kind, from_day, to_day)
            .map_err(msg)
    }

    pub fn export_report(&mut self, kind_index: i32, from: &str, to: &str) -> Result<String, String> {
        let kind = ReportKind::from_index(kind_index);
        let table = self.report_table(kind_index, from, to)?;

        let dir = storage_paths().exports;
        fs::create_dir_all(&dir).map_err(msg)?;
        let stamp = self.now().format("%Y%m%d-%H%M%S").to_string();
        let path = dir.join(format!("relatorio-{}-{stamp}.csv", kind.slug()));
        fs::write(&path, to_csv(&table)).map_err(msg)?;
        Ok(format!("Relatório exportado para {}", path.display()))
    }

    /// Campos vazios assumem o mês atual até hoje. Relatórios de situação atual não usam o período.
    fn report_period(&self, kind: ReportKind, from: &str, to: &str) -> Result<(NaiveDate, NaiveDate), String> {
        let today = self.now().date();
        let first_day = today.with_day(1).unwrap_or(today);
        if !kind.uses_period() {
            return Ok((first_day, today));
        }
        let from_day = match from.trim() {
            "" => first_day,
            raw => parse_day("data inicial", raw)?,
        };
        let to_day = match to.trim() {
            "" => today,
            raw => parse_day("data final", raw)?,
        };
        Ok((from_day, to_day))
    }
}

fn parse_day(field: &str, raw: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(raw.trim(), "%d/%m/%Y")
        .map_err(|_| format!("informe '{field}' no formato dd/mm/aaaa"))
}