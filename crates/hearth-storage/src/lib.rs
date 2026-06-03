//! sqlx + SQLite storage for Hearth.
//!
//! Every personal-data row is scoped by `(platform, account_id)`:
//!
//! - `platform` matches CIG's launcher `platform_id` ('prod' vs 'ptu')
//!   so test-shard progress never pollutes the persistent universe.
//! - `account_id` is the Hearth-local UUIDv7 PK on the `accounts`
//!   table — keyed on the RSI account, so multi-account desktops keep
//!   per-account state cleanly separated.
//!
//! Storage commands take a `Scope` carrying both. Account rows are
//! upserted by handle (bootstrap reads it from the launcher store).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use hearth_core::{Account, OwnedBlueprint, Platform, RecordId, WishIntent, WishlistEntry};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};

/// Concrete connection-pool type used across the storage API.
pub type DbPool = Pool<Sqlite>;

/// Identity tuple stamped on every personal-data row. Cheap by-value.
#[derive(Debug, Clone, Copy)]
pub struct Scope {
    pub platform: Platform,
    pub account_id: RecordId,
}

impl Scope {
    pub fn new(platform: Platform, account_id: RecordId) -> Self {
        Self {
            platform,
            account_id,
        }
    }
}

/// Open an on-disk SQLite database at `path`, creating it if missing,
/// and run pending migrations.
pub async fn open(path: &Path) -> Result<DbPool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    // Per-connection options (vs. a one-off `PRAGMA` on the pool, which would
    // configure only one of the connections): FK enforcement on every
    // connection, WAL so a reader never blocks the writer, and a busy_timeout
    // so a brief write contention waits instead of erroring with SQLITE_BUSY.
    // Building from `SqliteConnectOptions` also sidesteps the fragile
    // `sqlite://C:\…?mode=rwc` URL string (backslashes + drive colon).
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .with_context(|| format!("opening sqlite at {}", path.display()))?;
    sqlx::migrate!()
        .run(&pool)
        .await
        .context("running migrations")?;
    Ok(pool)
}

/// In-memory pool for tests. Each call gets its own isolated DB.
pub async fn open_in_memory() -> Result<DbPool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .context("opening in-memory sqlite")?;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .context("enabling foreign_keys pragma")?;
    sqlx::migrate!().run(&pool).await.context("migrating")?;
    Ok(pool)
}

// ── Accounts ────────────────────────────────────────────────────────────────

/// Look up an existing account row by RSI handle, or insert a fresh one
/// with just `(id, handle, created_at)`. Anchors (`citizen_record`,
/// `enlisted`, `last_verified`) stay NULL until a profile-verify call
/// fills them. Returns the resulting `Account`.
///
/// Idempotent — calling twice with the same handle returns the same row.
pub async fn upsert_account_by_handle(pool: &DbPool, handle: &str) -> Result<Account> {
    if let Some(existing) = get_account_by_handle(pool, handle).await? {
        return Ok(existing);
    }
    // A prior rename may have recorded this handle as an alias (the user
    // merged the renamed account in the Accounts UI). Resolve to that account
    // rather than creating a duplicate — this is what makes the manual
    // migration stick across launches.
    if let Some(account_id) = alias_account_id(pool, handle).await?
        && let Some(account) = get_account(pool, account_id).await?
    {
        return Ok(account);
    }
    let id = RecordId::new_v7();
    let created_at = Utc::now();
    // `ON CONFLICT DO NOTHING` makes this race-safe: several startup paths
    // (warmup export, the sidebar/onboarding `active_scope`, the rename check)
    // can call this concurrently for the same handle. Without it, all of them
    // pass the `get_account_by_handle` check above, then collide on the
    // `accounts.handle` UNIQUE constraint — one wins, the rest error out.
    let inserted = sqlx::query(
        "INSERT INTO accounts (id, handle, created_at) VALUES (?, ?, ?) \
         ON CONFLICT(handle) DO NOTHING",
    )
    .bind(id.to_string())
    .bind(handle)
    .bind(created_at.to_rfc3339())
    .execute(pool)
    .await
    .context("inserting accounts")?;

    if inserted.rows_affected() == 0 {
        // Lost the race: a concurrent caller inserted this handle first (under
        // its own id). Return the winning row rather than the one we'd have made.
        return get_account_by_handle(pool, handle)
            .await?
            .context("account row missing after ON CONFLICT DO NOTHING");
    }

    Ok(Account {
        id,
        handle: handle.to_string(),
        citizen_record: None,
        enlisted: None,
        last_verified: None,
        account_hint: None,
        created_at,
    })
}

pub async fn get_account(pool: &DbPool, id: RecordId) -> Result<Option<Account>> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, handle, citizen_record, enlisted, last_verified, account_hint, created_at \
         FROM accounts WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .context("selecting accounts by id")?;
    row.map(row_to_account).transpose()
}

pub async fn get_account_by_handle(pool: &DbPool, handle: &str) -> Result<Option<Account>> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, handle, citizen_record, enlisted, last_verified, account_hint, created_at \
         FROM accounts WHERE handle = ?",
    )
    .bind(handle)
    .fetch_optional(pool)
    .await
    .context("selecting accounts by handle")?;
    row.map(row_to_account).transpose()
}

pub async fn list_accounts(pool: &DbPool) -> Result<Vec<Account>> {
    let rows: Vec<AccountRow> = sqlx::query_as(
        "SELECT id, handle, citizen_record, enlisted, last_verified, account_hint, created_at \
         FROM accounts ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing accounts")?;
    rows.into_iter().map(row_to_account).collect()
}

/// Update the profile-derived anchors on an account row. Called after
/// a successful `/citizens/<handle>` scrape.
pub async fn update_account_anchors(
    pool: &DbPool,
    account_id: RecordId,
    citizen_record: i64,
    enlisted: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE accounts SET citizen_record = ?, enlisted = ?, last_verified = ? WHERE id = ?",
    )
    .bind(citizen_record)
    .bind(enlisted)
    .bind(Utc::now().to_rfc3339())
    .bind(account_id.to_string())
    .execute(pool)
    .await
    .context("updating account anchors")?;
    Ok(())
}

/// Create a fresh account with an explicit handle + optional numeric
/// `accountId` hint. The "this is a separate account" classification path
/// in the history import. Errors if the handle is already a current handle.
pub async fn create_account(
    pool: &DbPool,
    handle: &str,
    account_hint: Option<i64>,
) -> Result<Account> {
    let id = RecordId::new_v7();
    let created_at = Utc::now();
    sqlx::query("INSERT INTO accounts (id, handle, account_hint, created_at) VALUES (?, ?, ?, ?)")
        .bind(id.to_string())
        .bind(handle)
        .bind(account_hint)
        .bind(created_at.to_rfc3339())
        .execute(pool)
        .await
        .context("inserting accounts (create_account)")?;
    Ok(Account {
        id,
        handle: handle.to_string(),
        citizen_record: None,
        enlisted: None,
        last_verified: None,
        account_hint,
        created_at,
    })
}

/// Resolve a handle — current or a recorded alias — to its account id.
pub async fn account_id_for_handle(pool: &DbPool, handle: &str) -> Result<Option<RecordId>> {
    if let Some(account) = get_account_by_handle(pool, handle).await? {
        return Ok(Some(account.id));
    }
    alias_account_id(pool, handle).await
}

async fn alias_account_id(pool: &DbPool, handle: &str) -> Result<Option<RecordId>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT account_id FROM account_aliases WHERE handle = ?")
            .bind(handle)
            .fetch_optional(pool)
            .await
            .context("selecting account_aliases by handle")?;
    match row {
        Some((id,)) => Ok(Some(RecordId(
            id.parse().context("parsing alias account_id")?,
        ))),
        None => Ok(None),
    }
}

/// Record a past handle for an account (rename history). Idempotent; moves
/// the handle to this account if it was previously aliased elsewhere. No-op
/// when the handle is already this account's current handle.
pub async fn add_account_alias(pool: &DbPool, account_id: RecordId, handle: &str) -> Result<()> {
    if let Some(account) = get_account(pool, account_id).await?
        && account.handle == handle
    {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO account_aliases (handle, account_id, added_at) VALUES (?, ?, ?) \
         ON CONFLICT(handle) DO UPDATE SET account_id = excluded.account_id, added_at = excluded.added_at",
    )
    .bind(handle)
    .bind(account_id.to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await
    .context("inserting account_aliases")?;
    Ok(())
}

/// Past handles recorded for an account, oldest-recorded first.
pub async fn list_account_aliases(pool: &DbPool, account_id: RecordId) -> Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT handle FROM account_aliases WHERE account_id = ? ORDER BY added_at")
            .bind(account_id.to_string())
            .fetch_all(pool)
            .await
            .context("listing account_aliases")?;
    Ok(rows.into_iter().map(|(h,)| h).collect())
}

/// Set the numeric `accountId` hint on an account (from a log session).
pub async fn set_account_hint(pool: &DbPool, account_id: RecordId, hint: i64) -> Result<()> {
    sqlx::query("UPDATE accounts SET account_hint = ? WHERE id = ?")
        .bind(hint)
        .bind(account_id.to_string())
        .execute(pool)
        .await
        .context("updating account_hint")?;
    Ok(())
}

/// Merge `from` into `into`: reassign owned + wishlist rows, fold `from`'s
/// handle and aliases into `into`'s aliases, carry the numeric hint if `into`
/// lacks one, then delete `from`. Atomic. No-op when `from == into`.
///
/// `UPDATE OR IGNORE` skips personal-data rows that would collide with one
/// `into` already has (same blueprint on the same platform / intent) — those
/// stale duplicates are then deleted with the rest of `from`'s rows.
pub async fn merge_accounts(pool: &DbPool, from: RecordId, into: RecordId) -> Result<()> {
    if from == into {
        return Ok(());
    }
    let (from_s, into_s) = (from.to_string(), into.to_string());
    let mut tx = pool.begin().await.context("beginning merge transaction")?;

    sqlx::query("UPDATE OR IGNORE owned_blueprints SET account_id = ? WHERE account_id = ?")
        .bind(&into_s)
        .bind(&from_s)
        .execute(&mut *tx)
        .await
        .context("reassigning owned")?;
    sqlx::query("DELETE FROM owned_blueprints WHERE account_id = ?")
        .bind(&from_s)
        .execute(&mut *tx)
        .await
        .context("dropping leftover owned")?;
    sqlx::query("UPDATE OR IGNORE wishlist_entries SET account_id = ? WHERE account_id = ?")
        .bind(&into_s)
        .bind(&from_s)
        .execute(&mut *tx)
        .await
        .context("reassigning wishlist")?;
    sqlx::query("DELETE FROM wishlist_entries WHERE account_id = ?")
        .bind(&from_s)
        .execute(&mut *tx)
        .await
        .context("dropping leftover wishlist")?;

    // Move from's aliases onto into.
    sqlx::query("UPDATE account_aliases SET account_id = ? WHERE account_id = ?")
        .bind(&into_s)
        .bind(&from_s)
        .execute(&mut *tx)
        .await
        .context("moving aliases")?;

    // Record from's current handle as an alias of into, and carry its hint
    // if into has none.
    let from_row: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT handle, account_hint FROM accounts WHERE id = ?")
            .bind(&from_s)
            .fetch_optional(&mut *tx)
            .await
            .context("fetching from account")?;
    if let Some((from_handle, from_hint)) = from_row {
        sqlx::query(
            "INSERT INTO account_aliases (handle, account_id, added_at) VALUES (?, ?, ?) \
             ON CONFLICT(handle) DO UPDATE SET account_id = excluded.account_id",
        )
        .bind(&from_handle)
        .bind(&into_s)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await
        .context("aliasing from handle")?;
        if let Some(hint) = from_hint {
            sqlx::query(
                "UPDATE accounts SET account_hint = ? WHERE id = ? AND account_hint IS NULL",
            )
            .bind(hint)
            .bind(&into_s)
            .execute(&mut *tx)
            .await
            .context("carrying hint")?;
        }
    }

    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(&from_s)
        .execute(&mut *tx)
        .await
        .context("deleting from account")?;
    // Guard against a stale self-alias (into's current handle in the alias table).
    sqlx::query(
        "DELETE FROM account_aliases WHERE account_id = ? \
         AND handle = (SELECT handle FROM accounts WHERE id = ?)",
    )
    .bind(&into_s)
    .bind(&into_s)
    .execute(&mut *tx)
    .await
    .context("cleaning self-alias")?;

    tx.commit().await.context("committing merge")?;
    Ok(())
}

/// Apply a confirmed handle rename: make `new_handle` the account's current
/// handle and demote the old one to a recorded former handle. If a separate
/// account row already carries `new_handle` (the eager bootstrap created one
/// from the launcher handle before the rename was detected), it's absorbed into
/// `account_id` first — so its data isn't lost and the `UNIQUE(handle)`
/// constraint stays satisfied. No-op when `new_handle` is already current.
///
/// Caller's job to confirm it really is the same account (the rename check does
/// this via the immutable citizen-record anchor); this is the storage mechanics.
pub async fn apply_rename(pool: &DbPool, account_id: RecordId, new_handle: &str) -> Result<()> {
    // Absorb a duplicate row that already grabbed the new handle — or
    // short-circuit if this account already holds it.
    if let Some(existing) = get_account_by_handle(pool, new_handle).await? {
        if existing.id == account_id {
            return Ok(());
        }
        merge_accounts(pool, existing.id, account_id).await?;
    }

    let old = get_account(pool, account_id)
        .await?
        .with_context(|| format!("apply_rename: account {account_id} not found"))?;
    if old.handle == new_handle {
        return Ok(());
    }

    let mut tx = pool.begin().await.context("beginning rename transaction")?;
    // Promote new_handle to current.
    sqlx::query("UPDATE accounts SET handle = ? WHERE id = ?")
        .bind(new_handle)
        .bind(account_id.to_string())
        .execute(&mut *tx)
        .await
        .context("updating account handle")?;
    // Demote the old handle to a former handle.
    sqlx::query(
        "INSERT INTO account_aliases (handle, account_id, added_at) VALUES (?, ?, ?) \
         ON CONFLICT(handle) DO UPDATE SET account_id = excluded.account_id, added_at = excluded.added_at",
    )
    .bind(&old.handle)
    .bind(account_id.to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await
    .context("recording former handle")?;
    // new_handle is now the current handle — it must not also sit in the alias
    // table (the merge above may have parked it there).
    sqlx::query("DELETE FROM account_aliases WHERE handle = ?")
        .bind(new_handle)
        .execute(&mut *tx)
        .await
        .context("clearing self-alias after rename")?;
    tx.commit().await.context("committing rename")?;
    Ok(())
}

type AccountRow = (
    String,         // id
    String,         // handle
    Option<i64>,    // citizen_record
    Option<String>, // enlisted
    Option<String>, // last_verified
    Option<i64>,    // account_hint
    String,         // created_at
);

fn row_to_account(row: AccountRow) -> Result<Account> {
    let (id, handle, citizen_record, enlisted, last_verified, account_hint, created_at) = row;
    Ok(Account {
        id: RecordId(id.parse().context("parsing account id")?),
        handle,
        citizen_record,
        enlisted,
        last_verified: last_verified
            .as_deref()
            .map(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .map(|d| d.with_timezone(&Utc))
                    .context("parsing last_verified")
            })
            .transpose()?,
        account_hint,
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
            .context("parsing created_at")?
            .with_timezone(&Utc),
    })
}

// ── Owned blueprints ────────────────────────────────────────────────────────

pub async fn add_owned(
    pool: &DbPool,
    scope: Scope,
    blueprint_guid: &str,
) -> Result<OwnedBlueprint> {
    // Domain invariant: owning a BP makes "want the blueprint" moot, so
    // clear any recipe-intent wishlist entry for it. Done unconditionally
    // (even on the idempotent already-owned path) so an inconsistent
    // owned+want-recipe state self-heals. The `item` intent is untouched —
    // you can own the BP and still want a crafted copy.
    remove_from_wishlist(pool, scope, blueprint_guid, WishIntent::Recipe).await?;
    if let Some(existing) = get_owned(pool, scope, blueprint_guid).await? {
        return Ok(existing);
    }
    let id = RecordId::new_v7();
    let owned_at = Utc::now();
    sqlx::query(
        "INSERT INTO owned_blueprints (id, blueprint_guid, platform_id, account_id, owned_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(blueprint_guid)
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .bind(owned_at.to_rfc3339())
    .execute(pool)
    .await
    .context("inserting owned_blueprints")?;
    Ok(OwnedBlueprint {
        id,
        blueprint_guid: blueprint_guid.to_string(),
        platform: scope.platform,
        account_id: scope.account_id,
        owned_at,
    })
}

pub async fn remove_owned(pool: &DbPool, scope: Scope, blueprint_guid: &str) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM owned_blueprints \
         WHERE blueprint_guid = ? AND platform_id = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .execute(pool)
    .await
    .context("deleting from owned_blueprints")?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_owned(
    pool: &DbPool,
    scope: Scope,
    blueprint_guid: &str,
) -> Result<Option<OwnedBlueprint>> {
    let row: Option<OwnedRow> = sqlx::query_as(
        "SELECT id, blueprint_guid, platform_id, account_id, owned_at FROM owned_blueprints \
         WHERE blueprint_guid = ? AND platform_id = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .fetch_optional(pool)
    .await
    .context("selecting owned_blueprints")?;
    row.map(row_to_owned).transpose()
}

pub async fn list_owned(pool: &DbPool, scope: Scope) -> Result<Vec<OwnedBlueprint>> {
    let rows: Vec<OwnedRow> = sqlx::query_as(
        "SELECT id, blueprint_guid, platform_id, account_id, owned_at FROM owned_blueprints \
         WHERE platform_id = ? AND account_id = ? ORDER BY owned_at",
    )
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .fetch_all(pool)
    .await
    .context("listing owned_blueprints")?;
    rows.into_iter().map(row_to_owned).collect()
}

type OwnedRow = (String, String, String, String, String);

fn row_to_owned(row: OwnedRow) -> Result<OwnedBlueprint> {
    let (id, blueprint_guid, platform_id, account_id, owned_at) = row;
    Ok(OwnedBlueprint {
        id: RecordId(id.parse().context("parsing record id")?),
        blueprint_guid,
        platform: Platform::from_str(&platform_id)
            .with_context(|| format!("unknown platform_id {platform_id:?}"))?,
        account_id: RecordId(account_id.parse().context("parsing account_id")?),
        owned_at: chrono::DateTime::parse_from_rfc3339(&owned_at)
            .context("parsing owned_at")?
            .with_timezone(&Utc),
    })
}

// ── Wishlist ────────────────────────────────────────────────────────────────

pub async fn add_to_wishlist(
    pool: &DbPool,
    scope: Scope,
    blueprint_guid: &str,
    intent: WishIntent,
) -> Result<WishlistEntry> {
    if let Some(existing) = get_wishlist_entry(pool, scope, blueprint_guid, intent).await? {
        return Ok(existing);
    }
    let id = RecordId::new_v7();
    let added_at = Utc::now();
    sqlx::query(
        "INSERT INTO wishlist_entries (id, blueprint_guid, intent, platform_id, account_id, added_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(blueprint_guid)
    .bind(intent.as_str())
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .bind(added_at.to_rfc3339())
    .execute(pool)
    .await
    .context("inserting wishlist_entries")?;
    Ok(WishlistEntry {
        id,
        blueprint_guid: blueprint_guid.to_string(),
        intent,
        platform: scope.platform,
        account_id: scope.account_id,
        added_at,
    })
}

pub async fn remove_from_wishlist(
    pool: &DbPool,
    scope: Scope,
    blueprint_guid: &str,
    intent: WishIntent,
) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM wishlist_entries \
         WHERE blueprint_guid = ? AND intent = ? AND platform_id = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(intent.as_str())
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .execute(pool)
    .await
    .context("deleting from wishlist_entries")?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_wishlist_entry(
    pool: &DbPool,
    scope: Scope,
    blueprint_guid: &str,
    intent: WishIntent,
) -> Result<Option<WishlistEntry>> {
    let row: Option<WishlistRow> = sqlx::query_as(
        "SELECT id, blueprint_guid, intent, platform_id, account_id, added_at FROM wishlist_entries \
         WHERE blueprint_guid = ? AND intent = ? AND platform_id = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(intent.as_str())
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .fetch_optional(pool)
    .await
    .context("selecting wishlist_entries")?;
    row.map(row_to_wishlist).transpose()
}

pub async fn list_wishlist(pool: &DbPool, scope: Scope) -> Result<Vec<WishlistEntry>> {
    let rows: Vec<WishlistRow> = sqlx::query_as(
        "SELECT id, blueprint_guid, intent, platform_id, account_id, added_at FROM wishlist_entries \
         WHERE platform_id = ? AND account_id = ? ORDER BY added_at",
    )
    .bind(scope.platform.as_str())
    .bind(scope.account_id.to_string())
    .fetch_all(pool)
    .await
    .context("listing wishlist_entries")?;
    rows.into_iter().map(row_to_wishlist).collect()
}

type WishlistRow = (String, String, String, String, String, String);

fn row_to_wishlist(row: WishlistRow) -> Result<WishlistEntry> {
    let (id, blueprint_guid, intent, platform_id, account_id, added_at) = row;
    Ok(WishlistEntry {
        id: RecordId(id.parse().context("parsing record id")?),
        blueprint_guid,
        intent: WishIntent::from_str(&intent)
            .with_context(|| format!("unknown wishlist intent {intent:?}"))?,
        platform: Platform::from_str(&platform_id)
            .with_context(|| format!("unknown platform_id {platform_id:?}"))?,
        account_id: RecordId(account_id.parse().context("parsing account_id")?),
        added_at: chrono::DateTime::parse_from_rfc3339(&added_at)
            .context("parsing added_at")?
            .with_timezone(&Utc),
    })
}

// ── App settings (key/value, app-global) ─────────────────────────────────────

/// Read an app-global setting by key. `None` if unset.
pub async fn get_setting(pool: &DbPool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("reading setting {key}"))?;
    Ok(row.map(|(v,)| v))
}

/// Upsert an app-global setting.
pub async fn set_setting(pool: &DbPool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .with_context(|| format!("writing setting {key}"))?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn account(pool: &DbPool, handle: &str) -> Account {
        upsert_account_by_handle(pool, handle).await.unwrap()
    }

    fn scope(account: &Account, platform: Platform) -> Scope {
        Scope::new(platform, account.id)
    }

    #[tokio::test]
    async fn migrations_run() {
        let _pool = open_in_memory().await.expect("open in-memory db");
    }

    #[tokio::test]
    async fn account_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "VeeLume").await;
        assert_eq!(a.handle, "VeeLume");
        assert_eq!(a.citizen_record, None);
        assert_eq!(a.enlisted, None);

        // Idempotent — same handle returns same id.
        let b = account(&pool, "VeeLume").await;
        assert_eq!(a.id, b.id);

        // Anchor update.
        update_account_anchors(&pool, a.id, 1196670, "2016-01-31")
            .await
            .unwrap();
        let refreshed = get_account(&pool, a.id).await.unwrap().unwrap();
        assert_eq!(refreshed.citizen_record, Some(1196670));
        assert_eq!(refreshed.enlisted.as_deref(), Some("2016-01-31"));
        assert!(refreshed.last_verified.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_upsert_same_handle_converges() {
        // Reproduces the cold-start race: several startup paths upsert the same
        // handle at once. Needs the real multi-connection pool — `open_in_memory`
        // is single-connection and would serialize the collision away. Pre-fix,
        // the losers hit `UNIQUE constraint failed: accounts.handle`; post-fix
        // (ON CONFLICT DO NOTHING + re-read) they all converge on one row.
        let dir = std::env::temp_dir().join(format!("hearth_upsert_race_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("hearth.db");
        let pool = open(&db).await.unwrap();

        let mut set = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let pool = pool.clone();
            set.spawn(async move { upsert_account_by_handle(&pool, "VeeLume").await });
        }
        let mut ids = std::collections::HashSet::new();
        while let Some(res) = set.join_next().await {
            let acct = res.unwrap().expect("concurrent upsert must not error");
            ids.insert(acct.id);
        }
        assert_eq!(ids.len(), 1, "all concurrent upserts must return the same row");
        assert_eq!(list_accounts(&pool).await.unwrap().len(), 1);

        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn owned_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "VeeLume").await;
        let s = scope(&a, Platform::Prod);

        let bp = add_owned(&pool, s, "abc123").await.unwrap();
        assert_eq!(bp.blueprint_guid, "abc123");
        assert_eq!(bp.platform, Platform::Prod);
        assert_eq!(bp.account_id, a.id);

        let fetched = get_owned(&pool, s, "abc123").await.unwrap().unwrap();
        assert_eq!(fetched.id, bp.id);

        // Idempotent within scope.
        let again = add_owned(&pool, s, "abc123").await.unwrap();
        assert_eq!(again.id, bp.id);

        assert_eq!(list_owned(&pool, s).await.unwrap().len(), 1);
        assert!(remove_owned(&pool, s, "abc123").await.unwrap());
        assert!(get_owned(&pool, s, "abc123").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn platform_scope_isolation() {
        // Same account, same BP, different platforms → independent rows.
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "VeeLume").await;
        let prod = scope(&a, Platform::Prod);
        let ptu = scope(&a, Platform::Ptu);

        add_owned(&pool, prod, "shared-bp").await.unwrap();
        add_owned(&pool, ptu, "shared-bp").await.unwrap();

        assert!(get_owned(&pool, prod, "shared-bp").await.unwrap().is_some());
        assert!(get_owned(&pool, ptu, "shared-bp").await.unwrap().is_some());
        assert_eq!(list_owned(&pool, prod).await.unwrap().len(), 1);
        assert_eq!(list_owned(&pool, ptu).await.unwrap().len(), 1);

        remove_owned(&pool, prod, "shared-bp").await.unwrap();
        assert!(get_owned(&pool, prod, "shared-bp").await.unwrap().is_none());
        assert!(get_owned(&pool, ptu, "shared-bp").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn account_scope_isolation() {
        // Same BP, same platform, different accounts → independent rows.
        // This is the multi-RSI-account-on-one-desktop guarantee.
        let pool = open_in_memory().await.unwrap();
        let a1 = account(&pool, "VeeLume").await;
        let a2 = account(&pool, "AltAccount").await;
        let s1 = scope(&a1, Platform::Prod);
        let s2 = scope(&a2, Platform::Prod);

        add_owned(&pool, s1, "shared-bp").await.unwrap();
        add_owned(&pool, s2, "shared-bp").await.unwrap();
        assert_eq!(list_owned(&pool, s1).await.unwrap().len(), 1);
        assert_eq!(list_owned(&pool, s2).await.unwrap().len(), 1);

        // Removing one account's ownership doesn't touch the other's.
        remove_owned(&pool, s1, "shared-bp").await.unwrap();
        assert!(get_owned(&pool, s1, "shared-bp").await.unwrap().is_none());
        assert!(get_owned(&pool, s2, "shared-bp").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn wishlist_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "VeeLume").await;
        let s = scope(&a, Platform::Prod);

        let entry = add_to_wishlist(&pool, s, "want-1", WishIntent::Recipe)
            .await
            .unwrap();
        assert_eq!(entry.blueprint_guid, "want-1");
        assert_eq!(entry.intent, WishIntent::Recipe);
        assert_eq!(list_wishlist(&pool, s).await.unwrap().len(), 1);

        // Idempotent within (bp, intent).
        let again = add_to_wishlist(&pool, s, "want-1", WishIntent::Recipe)
            .await
            .unwrap();
        assert_eq!(again.id, entry.id);

        assert!(
            remove_from_wishlist(&pool, s, "want-1", WishIntent::Recipe)
                .await
                .unwrap()
        );
        assert!(list_wishlist(&pool, s).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn wishlist_intents_independent() {
        // The two intents coexist as separate rows for one BP.
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "VeeLume").await;
        let s = scope(&a, Platform::Prod);

        add_to_wishlist(&pool, s, "bp", WishIntent::Recipe)
            .await
            .unwrap();
        add_to_wishlist(&pool, s, "bp", WishIntent::Item)
            .await
            .unwrap();
        assert_eq!(list_wishlist(&pool, s).await.unwrap().len(), 2);

        // Removing one intent leaves the other.
        remove_from_wishlist(&pool, s, "bp", WishIntent::Recipe)
            .await
            .unwrap();
        assert!(
            get_wishlist_entry(&pool, s, "bp", WishIntent::Recipe)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            get_wishlist_entry(&pool, s, "bp", WishIntent::Item)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn owning_clears_want_blueprint() {
        // Owning a BP clears the recipe-intent wishlist (moot once owned)
        // but leaves the item intent (you can own the BP and still want a
        // crafted copy).
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "VeeLume").await;
        let s = scope(&a, Platform::Prod);

        add_to_wishlist(&pool, s, "bp", WishIntent::Recipe)
            .await
            .unwrap();
        add_to_wishlist(&pool, s, "bp", WishIntent::Item)
            .await
            .unwrap();

        add_owned(&pool, s, "bp").await.unwrap();
        assert!(
            get_wishlist_entry(&pool, s, "bp", WishIntent::Recipe)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            get_wishlist_entry(&pool, s, "bp", WishIntent::Item)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn alias_resolves_and_bootstrap_follows_it() {
        // A renamed account: current handle "NewName", past handle "OldName".
        let pool = open_in_memory().await.unwrap();
        let a = account(&pool, "NewName").await;
        add_account_alias(&pool, a.id, "OldName").await.unwrap();

        assert_eq!(
            list_account_aliases(&pool, a.id).await.unwrap(),
            vec!["OldName"]
        );
        assert_eq!(
            account_id_for_handle(&pool, "OldName").await.unwrap(),
            Some(a.id)
        );
        assert_eq!(
            account_id_for_handle(&pool, "NewName").await.unwrap(),
            Some(a.id)
        );
        assert_eq!(
            account_id_for_handle(&pool, "Stranger").await.unwrap(),
            None
        );

        // Bootstrapping by the old handle resolves to the same account — no
        // duplicate row (the key guarantee for the rename migration).
        let again = upsert_account_by_handle(&pool, "OldName").await.unwrap();
        assert_eq!(again.id, a.id);
        assert_eq!(list_accounts(&pool).await.unwrap().len(), 1);

        // Aliasing an account's own current handle is a no-op.
        add_account_alias(&pool, a.id, "NewName").await.unwrap();
        assert_eq!(
            list_account_aliases(&pool, a.id).await.unwrap(),
            vec!["OldName"]
        );
    }

    #[tokio::test]
    async fn merge_reassigns_owned_and_aliases_old_handle() {
        // Two account rows split by a rename; merge OldName → NewName.
        let pool = open_in_memory().await.unwrap();
        let from = account(&pool, "OldName").await;
        let into = account(&pool, "NewName").await;
        let sf = scope(&from, Platform::Prod);
        let si = scope(&into, Platform::Prod);

        add_owned(&pool, sf, "bp-shared").await.unwrap();
        add_owned(&pool, sf, "bp-only-old").await.unwrap();
        add_owned(&pool, si, "bp-shared").await.unwrap(); // same BP both accounts

        merge_accounts(&pool, from.id, into.id).await.unwrap();

        // `from` is gone; `into` owns the union with the shared BP deduped.
        assert!(get_account(&pool, from.id).await.unwrap().is_none());
        let mut guids: Vec<_> = list_owned(&pool, si)
            .await
            .unwrap()
            .into_iter()
            .map(|o| o.blueprint_guid)
            .collect();
        guids.sort();
        assert_eq!(guids, vec!["bp-only-old", "bp-shared"]);

        // OldName is now an alias of the survivor and resolves to it; a future
        // bootstrap under either handle lands on the same account.
        assert!(
            list_account_aliases(&pool, into.id)
                .await
                .unwrap()
                .contains(&"OldName".to_string())
        );
        assert_eq!(
            account_id_for_handle(&pool, "OldName").await.unwrap(),
            Some(into.id)
        );
        assert_eq!(
            upsert_account_by_handle(&pool, "OldName").await.unwrap().id,
            into.id
        );
    }

    #[tokio::test]
    async fn apply_rename_swaps_handle_and_absorbs_dupe() {
        // Established account under the old handle, with data + anchor, plus an
        // empty row the eager bootstrap created under the new handle.
        let pool = open_in_memory().await.unwrap();
        let x = account(&pool, "OldName").await;
        update_account_anchors(&pool, x.id, 4242, "2016-01-31")
            .await
            .unwrap();
        add_owned(&pool, scope(&x, Platform::Prod), "bp-x")
            .await
            .unwrap();
        let dupe = account(&pool, "NewName").await;
        assert_ne!(dupe.id, x.id);

        apply_rename(&pool, x.id, "NewName").await.unwrap();

        // One account survives: current handle NewName, OldName demoted, anchor
        // and data preserved, both handles resolve to it.
        assert_eq!(list_accounts(&pool).await.unwrap().len(), 1);
        let survivor = get_account(&pool, x.id).await.unwrap().unwrap();
        assert_eq!(survivor.handle, "NewName");
        assert_eq!(survivor.citizen_record, Some(4242));
        assert_eq!(
            list_account_aliases(&pool, x.id).await.unwrap(),
            vec!["OldName"]
        );
        assert_eq!(
            account_id_for_handle(&pool, "OldName").await.unwrap(),
            Some(x.id)
        );
        assert_eq!(
            account_id_for_handle(&pool, "NewName").await.unwrap(),
            Some(x.id)
        );
        assert_eq!(
            list_owned(&pool, scope(&survivor, Platform::Prod))
                .await
                .unwrap()
                .len(),
            1
        );

        // Idempotent — re-applying the current handle changes nothing.
        apply_rename(&pool, x.id, "NewName").await.unwrap();
        assert_eq!(
            list_account_aliases(&pool, x.id).await.unwrap(),
            vec!["OldName"]
        );
    }

    #[tokio::test]
    async fn apply_rename_without_a_dupe() {
        // No pre-existing row for the new handle — a plain promote/demote.
        let pool = open_in_memory().await.unwrap();
        let x = account(&pool, "OldName").await;
        apply_rename(&pool, x.id, "FreshName").await.unwrap();
        let survivor = get_account(&pool, x.id).await.unwrap().unwrap();
        assert_eq!(survivor.handle, "FreshName");
        assert_eq!(
            list_account_aliases(&pool, x.id).await.unwrap(),
            vec!["OldName"]
        );
        assert_eq!(
            account_id_for_handle(&pool, "FreshName").await.unwrap(),
            Some(x.id)
        );
    }
}
