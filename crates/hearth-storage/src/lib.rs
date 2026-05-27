//! sqlx + SQLite storage for Hearth.
//!
//! All personal-data API functions take a `Scope` (channel_group +
//! account_id) so the same physical DB cleanly separates PU state from
//! test-shard state, and is forward-compatible with multi-account
//! desktops once we wire RSI-handle detection.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use hearth_core::{ChannelGroup, OwnedBlueprint, RecordId, WishlistEntry};
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};

/// Concrete connection-pool type used across the storage API.
pub type DbPool = Pool<Sqlite>;

/// Identity tuple stamped on every personal-data row. Cheap by-value.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub channel_group: ChannelGroup,
    /// Empty string for the v1 default (no account detection yet).
    pub account_id: &'a str,
}

impl<'a> Scope<'a> {
    pub fn new(channel_group: ChannelGroup, account_id: &'a str) -> Self {
        Self {
            channel_group,
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
    sqlx::migrate!().run(&pool).await.context("migrating")?;
    Ok(pool)
}

// ── Owned blueprints ────────────────────────────────────────────────────────

pub async fn add_owned(
    pool: &DbPool,
    scope: Scope<'_>,
    blueprint_guid: &str,
) -> Result<OwnedBlueprint> {
    if let Some(existing) = get_owned(pool, scope, blueprint_guid).await? {
        return Ok(existing);
    }
    let id = RecordId::new_v7();
    let owned_at = Utc::now();
    sqlx::query(
        "INSERT INTO owned_blueprints (id, blueprint_guid, channel_group, account_id, owned_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(blueprint_guid)
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .bind(owned_at.to_rfc3339())
    .execute(pool)
    .await
    .context("inserting owned_blueprints")?;
    Ok(OwnedBlueprint {
        id,
        blueprint_guid: blueprint_guid.to_string(),
        channel_group: scope.channel_group,
        account_id: scope.account_id.to_string(),
        owned_at,
    })
}

pub async fn remove_owned(
    pool: &DbPool,
    scope: Scope<'_>,
    blueprint_guid: &str,
) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM owned_blueprints \
         WHERE blueprint_guid = ? AND channel_group = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .execute(pool)
    .await
    .context("deleting from owned_blueprints")?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_owned(
    pool: &DbPool,
    scope: Scope<'_>,
    blueprint_guid: &str,
) -> Result<Option<OwnedBlueprint>> {
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, channel_group, account_id, owned_at FROM owned_blueprints \
         WHERE blueprint_guid = ? AND channel_group = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .fetch_optional(pool)
    .await
    .context("selecting owned_blueprints")?;
    row.map(row_to_owned).transpose()
}

pub async fn list_owned(pool: &DbPool, scope: Scope<'_>) -> Result<Vec<OwnedBlueprint>> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, channel_group, account_id, owned_at FROM owned_blueprints \
         WHERE channel_group = ? AND account_id = ? ORDER BY owned_at",
    )
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .fetch_all(pool)
    .await
    .context("listing owned_blueprints")?;
    rows.into_iter().map(row_to_owned).collect()
}

fn row_to_owned(row: (String, String, String, String, String)) -> Result<OwnedBlueprint> {
    let (id, blueprint_guid, channel_group, account_id, owned_at) = row;
    Ok(OwnedBlueprint {
        id: RecordId(id.parse().context("parsing record id")?),
        blueprint_guid,
        channel_group: ChannelGroup::from_str(&channel_group)
            .with_context(|| format!("unknown channel_group {channel_group:?}"))?,
        account_id,
        owned_at: chrono::DateTime::parse_from_rfc3339(&owned_at)
            .context("parsing owned_at")?
            .with_timezone(&Utc),
    })
}

// ── Wishlist ────────────────────────────────────────────────────────────────

pub async fn add_to_wishlist(
    pool: &DbPool,
    scope: Scope<'_>,
    blueprint_guid: &str,
) -> Result<WishlistEntry> {
    if let Some(existing) = get_wishlist_entry(pool, scope, blueprint_guid).await? {
        return Ok(existing);
    }
    let id = RecordId::new_v7();
    let added_at = Utc::now();
    sqlx::query(
        "INSERT INTO wishlist_entries (id, blueprint_guid, channel_group, account_id, added_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(blueprint_guid)
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .bind(added_at.to_rfc3339())
    .execute(pool)
    .await
    .context("inserting wishlist_entries")?;
    Ok(WishlistEntry {
        id,
        blueprint_guid: blueprint_guid.to_string(),
        channel_group: scope.channel_group,
        account_id: scope.account_id.to_string(),
        added_at,
    })
}

pub async fn remove_from_wishlist(
    pool: &DbPool,
    scope: Scope<'_>,
    blueprint_guid: &str,
) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM wishlist_entries \
         WHERE blueprint_guid = ? AND channel_group = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .execute(pool)
    .await
    .context("deleting from wishlist_entries")?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_wishlist_entry(
    pool: &DbPool,
    scope: Scope<'_>,
    blueprint_guid: &str,
) -> Result<Option<WishlistEntry>> {
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, channel_group, account_id, added_at FROM wishlist_entries \
         WHERE blueprint_guid = ? AND channel_group = ? AND account_id = ?",
    )
    .bind(blueprint_guid)
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .fetch_optional(pool)
    .await
    .context("selecting wishlist_entries")?;
    row.map(row_to_wishlist).transpose()
}

pub async fn list_wishlist(pool: &DbPool, scope: Scope<'_>) -> Result<Vec<WishlistEntry>> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, channel_group, account_id, added_at FROM wishlist_entries \
         WHERE channel_group = ? AND account_id = ? ORDER BY added_at",
    )
    .bind(scope.channel_group.as_str())
    .bind(scope.account_id)
    .fetch_all(pool)
    .await
    .context("listing wishlist_entries")?;
    rows.into_iter().map(row_to_wishlist).collect()
}

fn row_to_wishlist(row: (String, String, String, String, String)) -> Result<WishlistEntry> {
    let (id, blueprint_guid, channel_group, account_id, added_at) = row;
    Ok(WishlistEntry {
        id: RecordId(id.parse().context("parsing record id")?),
        blueprint_guid,
        channel_group: ChannelGroup::from_str(&channel_group)
            .with_context(|| format!("unknown channel_group {channel_group:?}"))?,
        account_id,
        added_at: chrono::DateTime::parse_from_rfc3339(&added_at)
            .context("parsing added_at")?
            .with_timezone(&Utc),
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pu() -> Scope<'static> {
        Scope::new(ChannelGroup::Pu, "")
    }
    fn test_shard() -> Scope<'static> {
        Scope::new(ChannelGroup::Test, "")
    }

    #[tokio::test]
    async fn migrations_run() {
        let _pool = open_in_memory().await.expect("open in-memory db");
    }

    #[tokio::test]
    async fn owned_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let bp = add_owned(&pool, pu(), "abc123").await.unwrap();
        assert_eq!(bp.blueprint_guid, "abc123");
        assert_eq!(bp.channel_group, ChannelGroup::Pu);

        let fetched = get_owned(&pool, pu(), "abc123").await.unwrap().unwrap();
        assert_eq!(fetched.id, bp.id);

        let listed = list_owned(&pool, pu()).await.unwrap();
        assert_eq!(listed.len(), 1);

        // Idempotent: same guid+scope twice = same id.
        let second = add_owned(&pool, pu(), "abc123").await.unwrap();
        assert_eq!(second.id, bp.id);

        assert!(remove_owned(&pool, pu(), "abc123").await.unwrap());
        assert!(get_owned(&pool, pu(), "abc123").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn scope_isolation() {
        // PU and Test ownership of the same BP are independent — no
        // cross-contamination, both can exist, removing one doesn't
        // touch the other.
        let pool = open_in_memory().await.unwrap();
        add_owned(&pool, pu(), "shared-bp").await.unwrap();
        add_owned(&pool, test_shard(), "shared-bp").await.unwrap();

        assert!(get_owned(&pool, pu(), "shared-bp").await.unwrap().is_some());
        assert!(
            get_owned(&pool, test_shard(), "shared-bp")
                .await
                .unwrap()
                .is_some()
        );

        assert_eq!(list_owned(&pool, pu()).await.unwrap().len(), 1);
        assert_eq!(list_owned(&pool, test_shard()).await.unwrap().len(), 1);

        // Remove from PU; Test still has it.
        remove_owned(&pool, pu(), "shared-bp").await.unwrap();
        assert!(get_owned(&pool, pu(), "shared-bp").await.unwrap().is_none());
        assert!(
            get_owned(&pool, test_shard(), "shared-bp")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn wishlist_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let entry = add_to_wishlist(&pool, pu(), "want-1").await.unwrap();
        assert_eq!(entry.blueprint_guid, "want-1");
        assert_eq!(list_wishlist(&pool, pu()).await.unwrap().len(), 1);
        assert!(remove_from_wishlist(&pool, pu(), "want-1").await.unwrap());
        assert!(list_wishlist(&pool, pu()).await.unwrap().is_empty());
    }
}
