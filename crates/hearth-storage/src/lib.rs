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

use anyhow::{Context, Result};
use chrono::Utc;
use hearth_core::{Account, OwnedBlueprint, Platform, RecordId, WishIntent, WishlistEntry};
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};

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
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .with_context(|| format!("opening sqlite at {}", path.display()))?;
    // FK enforcement is off by default in SQLite.
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .context("enabling foreign_keys pragma")?;
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
    let id = RecordId::new_v7();
    let created_at = Utc::now();
    sqlx::query("INSERT INTO accounts (id, handle, created_at) VALUES (?, ?, ?)")
        .bind(id.to_string())
        .bind(handle)
        .bind(created_at.to_rfc3339())
        .execute(pool)
        .await
        .context("inserting accounts")?;
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
}
