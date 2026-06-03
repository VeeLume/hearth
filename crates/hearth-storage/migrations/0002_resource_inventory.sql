-- Live resource inventory (v1.5).
--
-- Authoritative snapshot of the player's stowed resources + discrete items,
-- read from CIG's backend via sc-dossier and resolved against the catalog at
-- sync time. One row per stack (a resource of a given quality in a given
-- place, or a discrete item stack). Scoped by (account_id, platform_id) like
-- every personal-data row; replaced wholesale on each sync (it's a snapshot,
-- not an incremental ledger — same posture as the authoritative blueprint
-- live sync).
--
-- `crc` is the EntityGraph wire id the stack is keyed on: the resource_id for
-- resource stacks, the item-class CRC for item stacks. It matches a recipe
-- ingredient's `crc`, so want-item coverage joins on it client-side.

CREATE TABLE resource_inventory (
    id              TEXT PRIMARY KEY,             -- UUIDv7
    account_id      TEXT NOT NULL
        REFERENCES accounts(id) ON DELETE CASCADE,
    platform_id     TEXT NOT NULL,
    kind            TEXT NOT NULL,                -- 'resource' or 'item'
    crc             INTEGER NOT NULL,             -- resource_id / class_crc
    name            TEXT,                         -- resolved display name (nullable)
    quality         INTEGER,                      -- resource only, 0..1000
    scu             REAL,                         -- resource only
    count           INTEGER,                      -- item only
    location_kind   TEXT NOT NULL,               -- 'player'|'location'|'hangar'|'container'|'entitlement'|'other'
    location_name   TEXT,                         -- resolved place name (nullable)
    container_geid  TEXT,                         -- owning ship/container geid (nullable)
    synced_at       TEXT NOT NULL
);

CREATE INDEX idx_resource_inventory_scope
    ON resource_inventory(account_id, platform_id);
