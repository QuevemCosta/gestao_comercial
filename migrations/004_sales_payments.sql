-- Migração 004: sequências de numeração, vendas, itens de venda e pagamentos.

CREATE TABLE sequences (
    name        TEXT    PRIMARY KEY CHECK (length(name) > 0),
    next_value  INTEGER NOT NULL CHECK (next_value >= 1)
) STRICT;

-- Um movimento só pode ser estornado uma vez.
CREATE UNIQUE INDEX ux_stock_movements_reversal
    ON stock_movements (reverses_movement_id)
    WHERE reverses_movement_id IS NOT NULL;

CREATE TABLE sales (
    id                    TEXT    PRIMARY KEY,
    number                TEXT    NOT NULL UNIQUE CHECK (length(number) > 0),
    order_id              TEXT    NOT NULL UNIQUE REFERENCES orders (id),
    customer_id           TEXT    NOT NULL REFERENCES customers (id),
    operational_status    TEXT    NOT NULL
                          CHECK (operational_status IN ('confirmed', 'completed', 'cancelled')),
    order_discount_cents  INTEGER NOT NULL DEFAULT 0 CHECK (order_discount_cents >= 0),
    cancelled_reason      TEXT,
    created_at            TEXT    NOT NULL,
    updated_at            TEXT    NOT NULL,
    completed_at          TEXT,
    CHECK ((operational_status = 'cancelled') = (cancelled_reason IS NOT NULL)),
    CHECK ((operational_status = 'completed') = (completed_at IS NOT NULL))
) STRICT;

CREATE INDEX idx_sales_customer ON sales (customer_id, created_at);
CREATE INDEX idx_sales_created ON sales (created_at);

CREATE TABLE sale_items (
    id                  TEXT    PRIMARY KEY,
    sale_id             TEXT    NOT NULL REFERENCES sales (id),
    position            INTEGER NOT NULL CHECK (position >= 0),
    product_id          TEXT    NOT NULL REFERENCES products (id),
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_cents    INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    discount_cents      INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0),
    stock_movement_id   TEXT    NOT NULL UNIQUE REFERENCES stock_movements (id),
    UNIQUE (sale_id, position),
    CHECK (discount_cents <= quantity * unit_price_cents)
) STRICT;

CREATE TABLE payments (
    id            TEXT    PRIMARY KEY,
    sale_id       TEXT    NOT NULL REFERENCES sales (id),
    method        TEXT    NOT NULL CHECK (method IN ('cash', 'pix', 'credit_card', 'debit_card')),
    amount_cents  INTEGER NOT NULL CHECK (amount_cents > 0),
    paid_at       TEXT    NOT NULL,
    notes         TEXT    NOT NULL DEFAULT ''
) STRICT;

CREATE INDEX idx_payments_sale ON payments (sale_id, paid_at);

-- Vendas, itens e pagamentos são registros históricos.
CREATE TRIGGER sales_no_delete
BEFORE DELETE ON sales
BEGIN
    SELECT RAISE(ABORT, 'vendas não podem ser apagadas');
END;

CREATE TRIGGER sale_items_no_update
BEFORE UPDATE ON sale_items
BEGIN
    SELECT RAISE(ABORT, 'itens de venda são imutáveis');
END;

CREATE TRIGGER sale_items_no_delete
BEFORE DELETE ON sale_items
BEGIN
    SELECT RAISE(ABORT, 'itens de venda não podem ser apagados');
END;

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
WHEN (SELECT operational_status FROM sales WHERE id = NEW.sale_id) = 'cancelled'
BEGIN
    SELECT RAISE(ABORT, 'venda cancelada não aceita pagamentos');
END;