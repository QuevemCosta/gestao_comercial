-- Migração 003: clientes, pedidos e itens de pedido.

CREATE TABLE customers (
    id            TEXT    PRIMARY KEY,
    person_type   TEXT    NOT NULL CHECK (person_type IN ('individual', 'company')),
    name          TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    document      TEXT    UNIQUE,
    phone         TEXT    CHECK (phone IS NULL OR length(phone) BETWEEN 10 AND 11),
    whatsapp      TEXT    CHECK (whatsapp IS NULL OR length(whatsapp) BETWEEN 10 AND 11),
    street        TEXT,
    neighborhood  TEXT,
    city          TEXT,
    notes         TEXT    NOT NULL DEFAULT '',
    active        INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at    TEXT    NOT NULL,
    updated_at    TEXT    NOT NULL,
    CHECK (
        document IS NULL
        OR (person_type = 'individual' AND length(document) = 11)
        OR (person_type = 'company' AND length(document) = 14)
    ),
    CHECK (phone IS NOT NULL OR whatsapp IS NOT NULL),
    CHECK (
        (street IS NULL AND neighborhood IS NULL AND city IS NULL)
        OR (street IS NOT NULL AND neighborhood IS NOT NULL AND city IS NOT NULL)
    )
) STRICT;

CREATE TABLE orders (
    id                    TEXT    PRIMARY KEY,
    customer_id           TEXT    NOT NULL REFERENCES customers (id),
    status                TEXT    NOT NULL DEFAULT 'draft'
                          CHECK (status IN ('draft', 'confirmed', 'converted', 'cancelled')),
    origin                TEXT    NOT NULL CHECK (origin IN ('whatsapp', 'in_person', 'other')),
    receive_mode          TEXT    NOT NULL CHECK (receive_mode IN ('delivery', 'pickup')),
    delivery_street       TEXT,
    delivery_neighborhood TEXT,
    delivery_city         TEXT,
    order_discount_cents  INTEGER NOT NULL DEFAULT 0 CHECK (order_discount_cents >= 0),
    notes                 TEXT    NOT NULL DEFAULT '',
    created_at            TEXT    NOT NULL,
    updated_at            TEXT    NOT NULL,
    CHECK (
        (receive_mode = 'delivery' AND delivery_street IS NOT NULL
            AND delivery_neighborhood IS NOT NULL AND delivery_city IS NOT NULL)
        OR
        (receive_mode = 'pickup' AND delivery_street IS NULL
            AND delivery_neighborhood IS NULL AND delivery_city IS NULL)
    )
) STRICT;

CREATE INDEX idx_orders_customer ON orders (customer_id, created_at);
CREATE INDEX idx_orders_status ON orders (status);

CREATE TABLE order_items (
    id                 TEXT    PRIMARY KEY,
    order_id           TEXT    NOT NULL REFERENCES orders (id),
    position           INTEGER NOT NULL CHECK (position >= 0),
    product_id         TEXT    NOT NULL REFERENCES products (id),
    quantity           INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_cents   INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    discount_cents     INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0),
    UNIQUE (order_id, product_id),
    UNIQUE (order_id, position),
    CHECK (discount_cents <= quantity * unit_price_cents)
) STRICT;

-- Itens só podem ser alterados enquanto o pedido estiver em elaboração.
-- A regra também é garantida no domínio; o trigger é uma segunda barreira.
CREATE TRIGGER order_items_insert_draft_only
BEFORE INSERT ON order_items
WHEN (SELECT status FROM orders WHERE id = NEW.order_id) <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'itens só podem ser incluídos em pedidos em elaboração');
END;

CREATE TRIGGER order_items_update_draft_only
BEFORE UPDATE ON order_items
WHEN (SELECT status FROM orders WHERE id = OLD.order_id) <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'itens só podem ser alterados em pedidos em elaboração');
END;

CREATE TRIGGER order_items_delete_draft_only
BEFORE DELETE ON order_items
WHEN (SELECT status FROM orders WHERE id = OLD.order_id) <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'itens só podem ser removidos de pedidos em elaboração');
END;