//! sqlx + SQLite storage for Hearth.
//!
//! Used by both the desktop client (`hearth-app`) as local cache + outbox
//! queue, and the v2 server (`hearth-server`) as canonical state — same
//! schema, same crate. Migrations live in `migrations/` and run via
//! `sqlx::migrate!()` at startup.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use hearth_core::{OwnedBlueprint, RecordId, WishlistEntry};
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};

/// Concrete connection-pool type used across the storage API. Keeping it
/// concrete (rather than generic over `Database`) is fine — we're SQLite
/// for the foreseeable future.
pub type DbPool = Pool<Sqlite>;

/// Open an on-disk SQLite database at `path`, creating it if missing, and
/// run pending migrations.
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

/// Mark a blueprint owned. Idempotent — second call with the same guid
/// returns the existing record.
pub async fn add_owned(pool: &DbPool, blueprint_guid: &str) -> Result<OwnedBlueprint> {
    if let Some(existing) = get_owned(pool, blueprint_guid).await? {
        return Ok(existing);
    }
    let id = RecordId::new_v7();
    let owned_at = Utc::now();
    let id_str = id.to_string();
    let owned_at_str = owned_at.to_rfc3339();
    sqlx::query("INSERT INTO owned_blueprints (id, blueprint_guid, owned_at) VALUES (?, ?, ?)")
        .bind(&id_str)
        .bind(blueprint_guid)
        .bind(&owned_at_str)
        .execute(pool)
        .await
        .context("inserting owned_blueprints")?;
    Ok(OwnedBlueprint {
        id,
        blueprint_guid: blueprint_guid.to_string(),
        owned_at,
    })
}

pub async fn remove_owned(pool: &DbPool, blueprint_guid: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM owned_blueprints WHERE blueprint_guid = ?")
        .bind(blueprint_guid)
        .execute(pool)
        .await
        .context("deleting from owned_blueprints")?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_owned(
    pool: &DbPool,
    blueprint_guid: &str,
) -> Result<Option<OwnedBlueprint>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, owned_at FROM owned_blueprints WHERE blueprint_guid = ?",
    )
    .bind(blueprint_guid)
    .fetch_optional(pool)
    .await
    .context("selecting owned_blueprints")?;
    row.map(row_to_owned).transpose()
}

pub async fn list_owned(pool: &DbPool) -> Result<Vec<OwnedBlueprint>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, owned_at FROM owned_blueprints ORDER BY owned_at",
    )
    .fetch_all(pool)
    .await
    .context("listing owned_blueprints")?;
    rows.into_iter().map(row_to_owned).collect()
}

fn row_to_owned(row: (String, String, String)) -> Result<OwnedBlueprint> {
    let (id, blueprint_guid, owned_at) = row;
    Ok(OwnedBlueprint {
        id: RecordId(id.parse().context("parsing record id")?),
        blueprint_guid,
        owned_at: chrono::DateTime::parse_from_rfc3339(&owned_at)
            .context("parsing owned_at")?
            .with_timezone(&Utc),
    })
}

// ── Wishlist ────────────────────────────────────────────────────────────────

pub async fn add_to_wishlist(pool: &DbPool, blueprint_guid: &str) -> Result<WishlistEntry> {
    if let Some(existing) = get_wishlist_entry(pool, blueprint_guid).await? {
        return Ok(existing);
    }
    let id = RecordId::new_v7();
    let added_at = Utc::now();
    let id_str = id.to_string();
    let added_at_str = added_at.to_rfc3339();
    sqlx::query("INSERT INTO wishlist_entries (id, blueprint_guid, added_at) VALUES (?, ?, ?)")
        .bind(&id_str)
        .bind(blueprint_guid)
        .bind(&added_at_str)
        .execute(pool)
        .await
        .context("inserting wishlist_entries")?;
    Ok(WishlistEntry {
        id,
        blueprint_guid: blueprint_guid.to_string(),
        added_at,
    })
}

pub async fn remove_from_wishlist(pool: &DbPool, blueprint_guid: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM wishlist_entries WHERE blueprint_guid = ?")
        .bind(blueprint_guid)
        .execute(pool)
        .await
        .context("deleting from wishlist_entries")?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_wishlist_entry(
    pool: &DbPool,
    blueprint_guid: &str,
) -> Result<Option<WishlistEntry>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, added_at FROM wishlist_entries WHERE blueprint_guid = ?",
    )
    .bind(blueprint_guid)
    .fetch_optional(pool)
    .await
    .context("selecting wishlist_entries")?;
    row.map(row_to_wishlist).transpose()
}

pub async fn list_wishlist(pool: &DbPool) -> Result<Vec<WishlistEntry>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, blueprint_guid, added_at FROM wishlist_entries ORDER BY added_at",
    )
    .fetch_all(pool)
    .await
    .context("listing wishlist_entries")?;
    rows.into_iter().map(row_to_wishlist).collect()
}

fn row_to_wishlist(row: (String, String, String)) -> Result<WishlistEntry> {
    let (id, blueprint_guid, added_at) = row;
    Ok(WishlistEntry {
        id: RecordId(id.parse().context("parsing record id")?),
        blueprint_guid,
        added_at: chrono::DateTime::parse_from_rfc3339(&added_at)
            .context("parsing added_at")?
            .with_timezone(&Utc),
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_run() {
        let _pool = open_in_memory().await.expect("open in-memory db");
    }

    #[tokio::test]
    async fn owned_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let bp = add_owned(&pool, "abc123").await.unwrap();
        assert_eq!(bp.blueprint_guid, "abc123");

        let fetched = get_owned(&pool, "abc123").await.unwrap().unwrap();
        assert_eq!(fetched.id, bp.id);

        let listed = list_owned(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);

        // Idempotent: same guid twice = same id.
        let second = add_owned(&pool, "abc123").await.unwrap();
        assert_eq!(second.id, bp.id);

        // Remove.
        assert!(remove_owned(&pool, "abc123").await.unwrap());
        assert!(get_owned(&pool, "abc123").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn wishlist_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        let entry = add_to_wishlist(&pool, "want-1").await.unwrap();
        assert_eq!(entry.blueprint_guid, "want-1");
        assert_eq!(list_wishlist(&pool).await.unwrap().len(), 1);
        assert!(remove_from_wishlist(&pool, "want-1").await.unwrap());
        assert!(list_wishlist(&pool).await.unwrap().is_empty());
    }
}

// ── Hint for offline sqlx macros ────────────────────────────────────────────
//
// We intentionally use the dynamic `sqlx::query` / `sqlx::query_as` APIs
// rather than the macro variants (`sqlx::query!`) because the latter require
// a live DATABASE_URL at compile time, which is hostile to a fresh clone.
// The trade-off is no compile-time SQL verification — for v1's small query
// set, the integration tests above cover the same ground.
