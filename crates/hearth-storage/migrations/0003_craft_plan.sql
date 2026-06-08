-- Crafting planner (v1.5): a plan of intended crafts, optionally grouped into
-- named projects. The reservation / coverage ledger (need vs have vs reserved
-- vs free vs short) is DERIVED client-side from these rows + the
-- `resource_inventory` snapshot — never stored, so it can't drift against an
-- inventory that is replaced wholesale on every sync.
--
-- Personal state: scoped by (account_id, platform_id) like every other row,
-- UUIDv7 ids. Additive migration — 0001 stays frozen, 0002 added the inventory.

-- Optional named groups (a loadout, an armour set). Pure grouping + rollup;
-- deleting one un-files its members (project_id -> NULL) rather than dropping
-- the planned crafts.
CREATE TABLE craft_projects (
    id           TEXT PRIMARY KEY,                 -- UUIDv7
    account_id   TEXT NOT NULL
        REFERENCES accounts(id) ON DELETE CASCADE,
    platform_id  TEXT NOT NULL,
    name         TEXT NOT NULL,
    notes        TEXT,
    sort_key     TEXT NOT NULL,                    -- manual ordering (lexicographic)
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE INDEX idx_craft_projects_scope ON craft_projects(account_id, platform_id);

-- One row per planned craft. `project_id` NULL => "Unsorted". The same
-- blueprint may appear in several rows (e.g. once per project) — there is no
-- scope-unique constraint, so multi-project planning falls out naturally. The
-- UUIDv7 `id` is time-sortable and doubles as the stable allocation tiebreak
-- within a horizon.
CREATE TABLE craft_plan_entries (
    id              TEXT PRIMARY KEY,              -- UUIDv7
    account_id      TEXT NOT NULL
        REFERENCES accounts(id) ON DELETE CASCADE,
    platform_id     TEXT NOT NULL,
    project_id      TEXT
        REFERENCES craft_projects(id) ON DELETE SET NULL,
    blueprint_guid  TEXT NOT NULL,
    quantity        INTEGER NOT NULL DEFAULT 1,    -- make N copies
    target_quality  INTEGER,                       -- 0..1000; NULL => Base (500)
    horizon         TEXT NOT NULL DEFAULT 'next',  -- now | next | later (alloc priority)
    notes           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_craft_plan_scope ON craft_plan_entries(account_id, platform_id);
