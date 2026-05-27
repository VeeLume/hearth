-- Initial Hearth schema (v1).
--
-- ID convention: UUIDv7 as TEXT (so SQLite sorts them naturally by creation time).
-- Timestamp convention: ISO 8601 UTC strings via TEXT. chrono + sqlx handle this.

CREATE TABLE owned_blueprints (
    id              TEXT PRIMARY KEY,
    blueprint_guid  TEXT NOT NULL UNIQUE,
    owned_at        TEXT NOT NULL
);

CREATE INDEX idx_owned_blueprints_blueprint_guid
    ON owned_blueprints(blueprint_guid);


CREATE TABLE mission_completions (
    id            TEXT PRIMARY KEY,
    mission_id    TEXT NOT NULL UNIQUE,
    completed_at  TEXT NOT NULL
);

CREATE INDEX idx_mission_completions_mission_id
    ON mission_completions(mission_id);


CREATE TABLE mission_rewards_collected (
    mission_completion_id  TEXT NOT NULL
        REFERENCES mission_completions(id) ON DELETE CASCADE,
    blueprint_guid         TEXT NOT NULL,
    PRIMARY KEY (mission_completion_id, blueprint_guid)
);


CREATE TABLE wishlist_entries (
    id              TEXT PRIMARY KEY,
    blueprint_guid  TEXT NOT NULL UNIQUE,
    added_at        TEXT NOT NULL
);

CREATE INDEX idx_wishlist_entries_blueprint_guid
    ON wishlist_entries(blueprint_guid);


-- Reserved for v2 sync. Empty + unused in v1. Schema stays stable from
-- day one so v1 → v2 doesn't require a destructive migration.
CREATE TABLE outbox (
    local_id         TEXT PRIMARY KEY,
    op               TEXT NOT NULL,
    payload          BLOB NOT NULL,
    attempt_count    INTEGER NOT NULL DEFAULT 0,
    last_attempt_at  TEXT,
    created_at       TEXT NOT NULL
);
