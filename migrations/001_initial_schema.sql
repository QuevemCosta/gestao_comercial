-- Migração 001: fundação do esquema.
-- Cria apenas a tabela de auditoria, usada pelas próximas etapas para registrar
-- operações importantes (Seção 8: histórico das operações).

CREATE TABLE audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TEXT    NOT NULL,
    action      TEXT    NOT NULL CHECK (length(action) > 0),
    entity      TEXT    NOT NULL CHECK (length(entity) > 0),
    entity_id   INTEGER,
    details     TEXT
) STRICT;