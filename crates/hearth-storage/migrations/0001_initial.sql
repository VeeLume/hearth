-- Initial Hearth schema (v1).
--
-- ID convention: UUIDv7 as TEXT (so SQLite sorts them naturally by creation time).
-- Timestamp convention: ISO 8601 UTC strings via TEXT.
--
-- Scope keys on every personal-data row:
--   - platform_id : 'prod' or 'ptu' — matches CIG's launcher-store
--                   platform_id field. 'prod' (Live + Hotfix) is the
--                   persistent universe; 'ptu' (PTU/EPTU/TechPreview)
--                   are test shards that wipe regularly. Strict
--                   separation prevents test progress polluting PU.
--   - account_id  : FK to accounts.id (UUIDv7 Hearth-local). One row
--                   per RSI account this desktop has seen.

CREATE TABLE accounts (
    id              TEXT PRIMARY KEY,             -- UUIDv7
    handle          TEXT NOT NULL UNIQUE,         -- current RSI handle
    citizen_record  INTEGER,                      -- public profile #
    enlisted        TEXT,                         -- public profile date
    last_verified   TEXT,                         -- last profile scrape ts
    account_hint    INTEGER,                      -- heapAccountId hint
    created_at      TEXT NOT NULL
);


CREATE TABLE owned_blueprints (
    id              TEXT PRIMARY KEY,
    blueprint_guid  TEXT NOT NULL,
    platform_id     TEXT NOT NULL,
    account_id      TEXT NOT NULL
        REFERENCES accounts(id) ON DELETE CASCADE,
    owned_at        TEXT NOT NULL,
    UNIQUE(blueprint_guid, platform_id, account_id)
);

CREATE INDEX idx_owned_blueprints_scope
    ON owned_blueprints(platform_id, account_id);


-- Mission completion is *derived*, not stored: a mission counts as
-- "exhausted" when the user owns every BP across its reward pool(s)
-- (computed from owned_blueprints). Hearth is blueprint-focused, so a
-- mission's only tracked value is "can it still give me a BP I lack?".
-- No mission_completions / mission_rewards_collected tables.

CREATE TABLE wishlist_entries (
    id              TEXT PRIMARY KEY,
    blueprint_guid  TEXT NOT NULL,
    intent          TEXT NOT NULL,                -- 'recipe' (want BP) or 'item' (want crafted item)
    platform_id     TEXT NOT NULL,
    account_id      TEXT NOT NULL
        REFERENCES accounts(id) ON DELETE CASCADE,
    added_at        TEXT NOT NULL,
    -- Both intents coexist for one BP, so intent is part of the key.
    UNIQUE(blueprint_guid, intent, platform_id, account_id)
);

CREATE INDEX idx_wishlist_entries_scope
    ON wishlist_entries(platform_id, account_id);


-- Reserved for v2 sync. Empty + unused in v1.
CREATE TABLE outbox (
    local_id         TEXT PRIMARY KEY,
    op               TEXT NOT NULL,
    payload          BLOB NOT NULL,
    attempt_count    INTEGER NOT NULL DEFAULT 0,
    last_attempt_at  TEXT,
    created_at       TEXT NOT NULL
);
