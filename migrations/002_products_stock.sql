-- Migração 002: categorias, produtos e movimentações de estoque.

CREATE TABLE categories (
    id          TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0),
    active      INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at  TEXT    NOT NULL
) STRICT;

CREATE TABLE products (
    id                    TEXT    PRIMARY KEY,
    sku                   TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(sku)) > 0),
    name                  TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    description           TEXT    NOT NULL DEFAULT '',
    category_id           TEXT    REFERENCES categories (id),
    purchase_price_cents  INTEGER NOT NULL CHECK (purchase_price_cents >= 0),
    sale_price_cents      INTEGER NOT NULL CHECK (sale_price_cents >= 0),
    stock_quantity        INTEGER NOT NULL DEFAULT 0 CHECK (stock_quantity >= 0),
    min_stock             INTEGER NOT NULL DEFAULT 0 CHECK (min_stock >= 0),
    unit                  TEXT    NOT NULL CHECK (length(trim(unit)) > 0),
    active                INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at            TEXT    NOT NULL,
    updated_at            TEXT    NOT NULL
) STRICT;

CREATE INDEX idx_products_category ON products (category_id);
CREATE INDEX idx_products_name ON products (name COLLATE NOCASE);

-- Sem DELETE: produtos com movimentações não podem ser apagados (ON DELETE padrão bloqueia).
CREATE TABLE stock_movements (
    id                    TEXT    PRIMARY KEY,
    product_id            TEXT    NOT NULL REFERENCES products (id),
    kind                  TEXT    NOT NULL CHECK (kind IN ('entry', 'exit', 'adjustment', 'reversal')),
    quantity_delta        INTEGER NOT NULL CHECK (quantity_delta <> 0),
    balance_after         INTEGER NOT NULL CHECK (balance_after >= 0),
    reason                TEXT    NOT NULL CHECK (length(trim(reason)) > 0),
    reverses_movement_id  TEXT    REFERENCES stock_movements (id),
    occurred_at           TEXT    NOT NULL,
    CHECK (
        (kind = 'entry' AND quantity_delta > 0) OR
        (kind = 'exit' AND quantity_delta < 0) OR
        (kind IN ('adjustment', 'reversal'))
    )
) STRICT;

CREATE INDEX idx_stock_movements_product ON stock_movements (product_id, occurred_at);

-- O histórico de movimentações é imutável (Seção 3.2 e 8: histórico das operações).
CREATE TRIGGER stock_movements_no_update
BEFORE UPDATE ON stock_movements
BEGIN
    SELECT RAISE(ABORT, 'movimentações de estoque são imutáveis');
END;

CREATE TRIGGER stock_movements_no_delete
BEFORE DELETE ON stock_movements
BEGIN
    SELECT RAISE(ABORT, 'movimentações de estoque não podem ser apagadas');
END;