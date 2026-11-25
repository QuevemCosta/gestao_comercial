use chrono::NaiveDateTime;

/// Registro de uma operação importante, guardado em `audit_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub occurred_at: NaiveDateTime,
    pub action: String,
    pub entity: String,
    pub details: String,
}