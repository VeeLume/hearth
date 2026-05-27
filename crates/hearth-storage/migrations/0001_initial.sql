-- Initial Hearth schema (v1).
--
-- ID convention: UUIDv7 as TEXT (so SQLite sorts them naturally by creation time).
-- Timestamp convention: ISO 8601 UTC strings via TEXT.
--
-- Scope keys on every personal-data row:
--   - channel_group : 'pu' or 'test'  — PU (Live + Hotfix) is persistent;
--                                       test shards (PTU/EPTU/TechPreview)
--                                       wipe regularly and must not pollute
--                                       PU state.
--   - account_id    : RSI handle the data belongs to. Empty string until
--                     account detection lands; reserved so multi-account
--                     on the same desktop doesn't need a future migration.

CREATE TABLE owned_blueprints (
    id              TEXT PRIMARY KEY,
    blueprint_guid  TEXT NOT NULL,
    channel_group   TEXT NOT NULL,
    account_id      TEXT NOT NULL DEFAULT '',
    owned_at        TEXT NOT NULL,
    UNIQUE(blueprint_guid, channel_group, account_id)
);

CREATE INDEX idx_owned_blueprints_scope
    ON owned_blueprints(channel_group, account_id);


CREATE TABLE mission_completions (
    id            TEXT PRIMARY KEY,
    mission_id    TEXT NOT NULL,
    channel_group TEXT NOT NULL,
    account_id    TEXT NOT NULL DEFAULT '',
    completed_at  TEXT NOT NULL,
    UNIQUE(mission_id, channel_group, account_id)
);

CREATE INDEX idx_mission_completions_scope
    ON mission_completions(channel_group, account_id);


CREATE TABLE mission_rewards_collected (
    mission_completion_id  TEXT NOT NULL
        REFERENCES mission_completions(id) ON DELETE CASCADE,
    blueprint_guid         TEXT NOT NULL,
    PRIMARY KEY (mission_completion_id, blueprint_guid)
);


CREATE TABLE wishlist_entries (
    id              TEXT PRIMARY KEY,
    blueprint_guid  TEXT NOT NULL,
    channel_group   TEXT NOT NULL,
    account_id      TEXT NOT NULL DEFAULT '',
    added_at        TEXT NOT NULL,
    UNIQUE(blueprint_guid, channel_group, account_id)
);

CREATE INDEX idx_wishlist_entries_scope
    ON wishlist_entries(channel_group, account_id);


-- Reserved for v2 sync. Empty + unused in v1.
CREATE TABLE outbox (
    local_id         TEXT PRIMARY KEY,
    op               TEXT NOT NULL,
    payload          BLOB NOT NULL,
    attempt_count    INTEGER NOT NULL DEFAULT 0,
    last_attempt_at  TEXT,
    created_at       TEXT NOT NULL
);
