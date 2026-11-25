use chrono::NaiveDateTime;

use super::errors::RepositoryError;
use super::payment::Payment;

/// Consultas necessárias apenas aos relatórios. Não alteram dados.
pub trait ReportRepository {
    /// Pagamentos de vendas e locações com data de recebimento em `[from, to)`.
    fn payments_between(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Payment>, RepositoryError>;
}