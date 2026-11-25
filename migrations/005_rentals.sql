-- Migração 005: itens de locação, locações, devoluções, pagamentos de locação e auditoria.

CREATE TABLE rental_items (
    id                 TEXT    PRIMARY KEY,
    name               TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    description        TEXT    NOT NULL DEFAULT '',
    category           TEXT    NOT NULL CHECK (length(trim(category)) > 0),
    total_quantity     INTEGER NOT NULL CHECK (total_quantity >= 0),
    rental_price_cents INTEGER NOT NULL CHECK (rental_price_cents >= 0),
    active             INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    notes              TEXT    NOT NULL DEFAULT '',
    created_at         TEXT    NOT NULL,
    updated_at         TEXT    NOT NULL
) STRICT;

CREATE TABLE rentals (
    id                 TEXT    PRIMARY KEY,
    number             TEXT    NOT NULL UNIQUE CHECK (length(number) > 0),
    customer_id        TEXT    NOT NULL REFERENCES customers (id),
    status             TEXT    NOT NULL CHECK (status IN ('reserved', 'active', 'closed', 'cancelled')),
    pickup_at          TEXT    NOT NULL,
    expected_return_at TEXT    NOT NULL,
    discount_cents     INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0),
    cancelled_reason   TEXT,
    picked_up_at       TEXT,
    closed_at          TEXT,
    created_at         TEXT    NOT NULL,
    updated_at         TEXT    NOT NULL,
    CHECK (pickup_at < expected_return_at),
    CHECK ((status = 'cancelled') = (cancelled_reason IS NOT NULL)),
    CHECK (status IN ('reserved', 'cancelled') OR picked_up_at IS NOT NULL),
    CHECK ((status = 'closed') = (closed_at IS NOT NULL))
) STRICT;

CREATE INDEX idx_rentals_customer ON rentals (customer_id, created_at);
CREATE INDEX idx_rentals_status ON rentals (status, pickup_at);

CREATE TABLE rental_lines (
    id                 TEXT    PRIMARY KEY,
    rental_id          TEXT    NOT NULL REFERENCES rentals (id),
    position           INTEGER NOT NULL CHECK (position >= 0),
    rental_item_id     TEXT    NOT NULL REFERENCES rental_items (id),
    quantity           INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_cents   INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    returned_quantity  INTEGER NOT NULL DEFAULT 0 CHECK (returned_quantity >= 0),
    damaged_quantity   INTEGER NOT NULL DEFAULT 0 CHECK (damaged_quantity >= 0),
    lost_quantity      INTEGER NOT NULL DEFAULT 0 CHECK (lost_quantity >= 0),
    UNIQUE (rental_id, position),
    UNIQUE (rental_id, rental_item_id),
    CHECK (returned_quantity + damaged_quantity + lost_quantity <= quantity)
) STRICT;

CREATE INDEX idx_rental_lines_item ON rental_lines (rental_item_id);

CREATE TABLE rental_return_events (
    id                 TEXT    PRIMARY KEY,
    rental_line_id     TEXT    NOT NULL REFERENCES rental_lines (id),
    occurred_at        TEXT    NOT NULL,
    returned_quantity  INTEGER NOT NULL CHECK (returned_quantity >= 0),
    damaged_quantity   INTEGER NOT NULL CHECK (damaged_quantity >= 0),
    lost_quantity      INTEGER NOT NULL CHECK (lost_quantity >= 0),
    notes              TEXT    NOT NULL DEFAULT '',
    CHECK (returned_quantity + damaged_quantity + lost_quantity > 0)
) STRICT;

CREATE INDEX idx_rental_return_events_line ON rental_return_events (rental_line_id);

CREATE TRIGGER rental_return_events_no_update
BEFORE UPDATE ON rental_return_events
BEGIN
    SELECT RAISE(ABORT, 'registros de devolução são imutáveis');
END;

CREATE TRIGGER rental_return_events_no_delete
BEFORE DELETE ON rental_return_events
BEGIN
    SELECT RAISE(ABORT, 'registros de devolução não podem ser apagados');
END;

-- Pagamentos passam a referenciar venda OU locação.
-- A tabela é recriada para tornar sale_id opcional; os dados existentes são preservados.
CREATE TABLE payments_new (
    id            TEXT    PRIMARY KEY,
    sale_id       TEXT    REFERENCES sales (id),
    rental_id     TEXT    REFERENCES rentals (id),
    method        TEXT    NOT NULL CHECK (method IN ('cash', 'pix', 'credit_card', 'debit_card')),
    amount_cents  INTEGER NOT NULL CHECK (amount_cents > 0),
    paid_at       TEXT    NOT NULL,
    notes         TEXT    NOT NULL DEFAULT '',
    CHECK ((sale_id IS NULL) <> (rental_id IS NULL))
) STRICT;

INSERT INTO payments_new (id, sale_id, rental_id, method, amount_cents, paid_at, notes)
    SELECT id, sale_id, NULL, method, amount_cents, paid_at, notes FROM payments;

DROP TABLE payments;
ALTER TABLE payments_new RENAME TO payments;

CREATE INDEX idx_payments_sale ON payments (sale_id, paid_at);
CREATE INDEX idx_payments_rental ON payments (rental_id, paid_at);

CREATE TRIGGER payments_no_update
BEFORE UPDATE ON payments
BEGIN
    SELECT RAISE(ABORT, 'pagamentos são imutáveis');
END;

CREATE TRIGGER payments_no_delete
BEFORE DELETE ON payments
BEGIN
    SELECT RAISE(ABORT, 'pagamentos não podem ser apagados');
END;

CREATE TRIGGER payments_only_open_sales
BEFORE INSERT ON payments
WHEN NEW.sale_id IS NOT NULL
 AND (SELECT operational_status FROM sales WHERE id = NEW.sale_id) = 'cancelled'
BEGIN
    SELECT RAISE(ABORT, 'venda cancelada não aceita pagamentos');
END;

CREATE TRIGGER payments_only_open_rentals
BEFORE INSERT ON payments
WHEN NEW.rental_id IS NOT NULL
 AND (SELECT status FROM rentals WHERE id = NEW.rental_id) = 'cancelled'
BEGIN
    SELECT RAISE(ABORT, 'locação cancelada não aceita pagamentos');
END;