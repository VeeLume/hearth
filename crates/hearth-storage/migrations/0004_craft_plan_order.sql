-- Crafting-planner refinements (still v1.5, additive):
--   * manual ordering of planned crafts (`sort_key`) — replaces the
--     Now/Next/Later horizon as the allocation-priority axis. The `horizon`
--     column from 0003 is left in place (NOT NULL DEFAULT 'next') but unused;
--     dropping it isn't worth a table rebuild for a feature that never shipped.
--   * a per-project "include in the plan" toggle (`active`) — an inactive
--     project drops out of both the materials rollup and the reservation pass.
--
-- `sort_key` backfills to the row id (UUIDv7 → creation order) so existing
-- entries keep a stable initial order; new rows and reorders set it explicitly.

ALTER TABLE craft_plan_entries ADD COLUMN sort_key TEXT NOT NULL DEFAULT '';
UPDATE craft_plan_entries SET sort_key = id WHERE sort_key = '';

ALTER TABLE craft_projects ADD COLUMN active INTEGER NOT NULL DEFAULT 1;
