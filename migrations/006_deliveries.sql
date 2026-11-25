-- Migração 006: entregadores, entregas, histórico de status, agendamentos e atribuições.

CREATE TABLE deliverers (
    id          TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    phone       TEXT    NOT NULL CHECK (length(phone) BETWEEN 10 AND 11),
    document    TEXT    UNIQUE CHECK (document IS NULL OR length(document) = 11),
    notes       TEXT    NOT NULL DEFAULT '',
    active      INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
) STRICT;

CREATE TABLE deliveries (
    id                        TEXT    PRIMARY KEY,
    order_id                  TEXT    NOT NULL UNIQUE REFERENCES orders (id),
    customer_id               TEXT    NOT NULL REFERENCES customers (id),
    status                    TEXT    NOT NULL CHECK (status IN (
                                  'awaiting_scheduling', 'scheduled', 'out_for_delivery',
                                  'delivered', 'not_delivered', 'cancelled')),
    destination_street        TEXT    NOT NULL CHECK (length(trim(destination_street)) > 0),
    destination_neighborhood  TEXT    NOT NULL CHECK (length(trim(destination_neighborhood)) > 0),
    destination_city          TEXT    NOT NULL CHECK (length(trim(destination_city)) > 0),
    contact_phone             TEXT    NOT NULL CHECK (length(contact_phone) BETWEEN 10 AND 11),
    fee_cents                 INTEGER NOT NULL DEFAULT 0 CHECK (fee_cents >= 0),
    notes                     TEXT    NOT NULL DEFAULT '',
    scheduled_for             TEXT,
    completed_at              TEXT,
    received_by               TEXT,
    failure_reason            TEXT,
    cancelled_reason          TEXT,
    created_at                TEXT    NOT NULL,
    updated_at                TEXT    NOT NULL,
    CHECK (status IN ('awaiting_scheduling', 'cancelled') OR scheduled_for IS NOT NULL),
    CHECK ((status = 'delivered') = (completed_at IS NOT NULL)),
    CHECK (status <> 'not_delivered' OR failure_reason IS NOT NULL),
    CHECK ((status = 'cancelled') = (cancelled_reason IS NOT NULL))
) STRICT;

CREATE INDEX idx_deliveries_scheduled ON deliveries (scheduled_for);
CREATE INDEX idx_deliveries_status ON deliveries (status);
CREATE INDEX idx_deliveries_customer ON deliveries (customer_id);

CREATE TABLE delivery_status_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    delivery_id  TEXT    NOT NULL REFERENCES deliveries (id),
    from_status  TEXT    CHECK (from_status IS NULL OR from_status IN (
                     'awaiting_scheduling', 'scheduled', 'out_for_delivery',
                     'delivered', 'not_delivered', 'cancelled')),
    to_status    TEXT    NOT NULL CHECK (to_status IN (
                     'awaiting_scheduling', 'scheduled', 'out_for_delivery',
                     'delivered', 'not_delivered', 'cancelled')),
    changed_at   TEXT    NOT NULL,
    note         TEXT    NOT NULL DEFAULT ''
) STRICT;

CREATE INDEX idx_delivery_status_history_delivery ON delivery_status_history (delivery_id, id);

CREATE TABLE delivery_schedules (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    delivery_id   TEXT    NOT NULL REFERENCES deliveries (id),
    scheduled_for TEXT    NOT NULL,
    recorded_at   TEXT    NOT NULL
) STRICT;

CREATE INDEX idx_delivery_schedules_delivery ON delivery_schedules (delivery_id, id);

CREATE TABLE delivery_assignments (
    id            TEXT    PRIMARY KEY,
    delivery_id   TEXT    NOT NULL REFERENCES deliveries (id),
    deliverer_id  TEXT    NOT NULL REFERENCES deliverers (id),
    assigned_at   TEXT    NOT NULL,
    unassigned_at TEXT,
    CHECK (unassigned_at IS NULL OR unassigned_at >= assigned_at)
) STRICT;

-- Uma entrega só pode ter um entregador ativo por vez.
CREATE UNIQUE INDEX ux_delivery_active_assignment
    ON delivery_assignments (delivery_id)
    WHERE unassigned_at IS NULL;

CREATE INDEX idx_delivery_assignments_deliverer
    ON delivery_assignments (deliverer_id, unassigned_at);

-- Histórico é imutável.
CREATE TRIGGER delivery_status_history_no_update
BEFORE UPDATE ON delivery_status_history
BEGIN
    SELECT RAISE(ABORT, 'histórico de status de entrega é imutável');
END;

CREATE TRIGGER delivery_status_history_no_delete
BEFORE DELETE ON delivery_status_history
BEGIN
    SELECT RAISE(ABORT, 'histórico de status de entrega não pode ser apagado');
END;

CREATE TRIGGER delivery_schedules_no_update
BEFORE UPDATE ON delivery_schedules
BEGIN
    SELECT RAISE(ABORT, 'agendamentos são imutáveis');
END;

CREATE TRIGGER delivery_schedules_no_delete
BEFORE DELETE ON delivery_schedules
BEGIN
    SELECT RAISE(ABORT, 'agendamentos não podem ser apagados');
END;

-- Atribuições podem ser encerradas uma única vez; nada mais muda.
CREATE TRIGGER delivery_assignments_update_guard
BEFORE UPDATE ON delivery_assignments
WHEN OLD.unassigned_at IS NOT NULL
  OR NEW.unassigned_at IS NULL
  OR NEW.delivery_id IS NOT OLD.delivery_id
  OR NEW.deliverer_id IS NOT OLD.deliverer_id
  OR NEW.assigned_at IS NOT OLD.assigned_at
BEGIN
    SELECT RAISE(ABORT, 'atribuições só podem ser encerradas uma vez');
END;

CREATE TRIGGER delivery_assignments_no_delete
BEFORE DELETE ON delivery_assignments
BEGIN
    SELECT RAISE(ABORT, 'atribuições não podem ser apagadas');
END;

CREATE TRIGGER delivery_assignments_active_deliverer
BEFORE INSERT ON delivery_assignments
WHEN (SELECT active FROM deliverers WHERE id = NEW.deliverer_id) = 0
BEGIN
    SELECT RAISE(ABORT, 'entregador inativo não pode receber entregas');
END;