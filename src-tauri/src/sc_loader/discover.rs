//! Stage 1 of the SC load — **fast** (~50ms): launcher-store reads.
//!
//! Finds the highest-priority install, derives the [`Platform`], reads the RSI
//! handle. Everything the sidebar's scope chip needs to render. Wraps the
//! synchronous `sc_holotable::install::*` calls in `spawn_blocking` so they
//! don't stall the tokio runtime. Never touches the DCB, so it needs no big
//! loader stack (unlike the catalog build in [`super::build_data`]).

use std::time::Instant;

use anyhow::{Context, Result};
use hearth_core::Platform;
use sc_holotable::install::{Channel, Installation};

/// Everything the sidebar / scope chip / DB-only commands need before
/// the heavy DCB parse starts. Produced by [`discover`] in ~50ms.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// Specific channel that produced this dataset (Live, Hotfix, …).
    pub channel: Channel,
    /// Stability grouping. `Prod` (Live + Hotfix) or `Ptu` (PTU, EPTU,
    /// TechPreview). Read from launcher store when available; falls
    /// back to a channel-based map otherwise.
    pub platform: Platform,
    /// RSI handle from the launcher store, if available. `None` if the
    /// launcher store couldn't be read or the identity block was empty
    /// (e.g. user has never signed into the launcher).
    pub handle: Option<String>,
    /// The chosen install. Held so [`super::build_data`] doesn't redo
    /// discovery, and so future stages (sensors / Game.log tailing) can
    /// resolve paths off it.
    pub install: Installation,
}

/// Find the highest-priority install, derive its [`Platform`], read the
/// launcher-store identity. Runs the synchronous launcher reads via
/// `spawn_blocking` so the tokio runtime stays responsive.
pub async fn discover() -> Result<Discovery> {
    tokio::task::spawn_blocking(discover_blocking)
        .await
        .context("joining discovery task")?
}

fn discover_blocking() -> Result<Discovery> {
    let start = Instant::now();
    let mut installs = sc_holotable::install::discover().context("sc discovery")?;
    if installs.is_empty() {
        anyhow::bail!("no Star Citizen installations detected");
    }
    installs.sort_by_key(|i| i.channel.priority());
    let install = installs.into_iter().next().expect("non-empty");
    let channel = install.channel;
    let platform = install
        .platform_id
        .as_deref()
        .and_then(Platform::from_str)
        .unwrap_or_else(|| group_for(channel));

    // Best-effort handle read. Failure here is logged but not fatal —
    // identity is bootstrapped from the launcher store when available;
    // sign-in flow / manual entry fills the gap otherwise.
    let handle = match sc_holotable::install::read_identity() {
        Ok(id) => Some(id.handle),
        Err(e) => {
            tracing::info!("launcher identity unavailable ({e}); handle stays unbound");
            None
        }
    };

    tracing::info!(
        channel = ?channel,
        platform = platform.as_str(),
        handle = ?handle,
        elapsed_ms = start.elapsed().as_millis(),
        "discovery complete"
    );
    Ok(Discovery {
        channel,
        platform,
        handle,
        install,
    })
}

/// Channel-based fallback when the launcher store didn't give us a
/// `platform_id` (e.g. log-fallback discovery). Mirrors CIG's own
/// platform mapping: Live + Hotfix → prod; everything else → ptu.
fn group_for(channel: Channel) -> Platform {
    match channel {
        Channel::Live | Channel::Hotfix => Platform::Prod,
        Channel::Ptu | Channel::Eptu | Channel::TechPreview => Platform::Ptu,
    }
}
