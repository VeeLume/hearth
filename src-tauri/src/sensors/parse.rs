//! Pure parsers for Star Citizen `Game.log` lines.
//!
//! Grounded in real log samples (see the tests — every fixture is a verbatim
//! line from a live `Game.log`). The log format is CIG-internal and churns
//! across patches, so these stay small, isolated, pure functions: a format
//! break is a localized fix here, never a rewrite of the tailing machinery.
//!
//! Only the handful of lines Hearth cares about are recognised; everything
//! else parses to `None`. Hearth's mission model is *derived from blueprint
//! ownership*, so the actionable signal is **"Received Blueprint"** — there's
//! no separate mission-completion line to track. The session-header lines
//! (env tag + handle) feed the pollution guard: only write owned blueprints
//! when the logged session matches the active account + platform.

use hearth_core::Platform;

/// A meaningful event sensed from one `Game.log` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SensedEvent {
    /// A blueprint was received (mission reward / comms grant). Carries the
    /// in-game **display name** — the log never includes the record guid, so
    /// the caller resolves name → `blueprint_record_guid` via the catalog.
    BlueprintReceived { name: String },
    /// The session's environment (`--envtag`), mapped to a [`Platform`].
    /// Appears once near the top of each session's log.
    SessionPlatform(Platform),
    /// The RSI handle signed in for this session (`Handle[...]`).
    SessionHandle(String),
    /// The numeric CIG `accountId` for this session (the `heapAccountId`).
    /// A handle rename keeps the same `accountId`, so it groups a renamed
    /// account's sessions for the history import's suggestions. Not an
    /// absolute key — CIG rotated it once (Oct–Dec 2024) — so the user
    /// confirms the grouping.
    SessionAccountId(i64),
}

/// Recognise one line. Returns `None` for the ~99% of lines that carry no
/// signal Hearth tracks.
pub fn parse_line(line: &str) -> Option<SensedEvent> {
    if let Some(name) = blueprint_received(line) {
        return Some(SensedEvent::BlueprintReceived { name: name.to_owned() });
    }
    if let Some(platform) = session_platform(line) {
        return Some(SensedEvent::SessionPlatform(platform));
    }
    if let Some(handle) = session_handle(line) {
        return Some(SensedEvent::SessionHandle(handle.to_owned()));
    }
    if let Some(id) = session_account_id(line) {
        return Some(SensedEvent::SessionAccountId(id));
    }
    None
}

/// The blueprint-received notification, e.g.:
///
/// ```text
/// <…> [Notice] <SHUDEvent_OnNotification> Added notification "Received Blueprint: Testudo Arms Clanguard: " [60] to queue. …
/// ```
///
/// Keyed on the **`Added notification`** form only. The same text recurs on
/// `<UpdateNotificationItem>` lifecycle lines (Next / StartFade / Remove) and
/// on the indented body echoes — those must NOT count as new acquisitions, so
/// the prefix deliberately includes the `<SHUDEvent_OnNotification>` source.
fn blueprint_received(line: &str) -> Option<&str> {
    const PREFIX: &str = "<SHUDEvent_OnNotification> Added notification \"Received Blueprint: ";
    let start = line.find(PREFIX)? + PREFIX.len();
    let rest = &line[start..];
    // The quoted text is `Received Blueprint: <name>: ` — the name runs up to
    // the closing `: "`. Item names contain no `"`, so the first `: "` is the
    // terminator even when the name itself contains a colon.
    let end = rest.find(": \"")?;
    let name = rest[..end].trim();
    (!name.is_empty()).then_some(name)
}

/// The session environment from the cmdline echo, e.g.:
///
/// ```text
/// <…>    [Cmdline* ] --envtag='PUB'
/// ```
///
/// `PUB` is the live persistent universe; every other tag (PTU / EPTU /
/// TechPreview) is a wiping test shard → [`Platform::Ptu`].
fn session_platform(line: &str) -> Option<Platform> {
    const PREFIX: &str = "--envtag='";
    let start = line.find(PREFIX)? + PREFIX.len();
    let rest = &line[start..];
    let end = rest.find('\'')?;
    let tag = &rest[..end];
    Some(if tag.eq_ignore_ascii_case("PUB") {
        Platform::Prod
    } else {
        Platform::Ptu
    })
}

/// The signed-in handle from the legacy login response, e.g.:
///
/// ```text
/// <…> [Notice] <Legacy login response> [CIG-net] User Login Success - Handle[VeeLume] - Time[…] …
/// ```
fn session_handle(line: &str) -> Option<&str> {
    if !line.contains("<Legacy login response>") {
        return None;
    }
    const PREFIX: &str = "Handle[";
    let start = line.find(PREFIX)? + PREFIX.len();
    let rest = &line[start..];
    let end = rest.find(']')?;
    let handle = &rest[..end];
    (!handle.is_empty()).then_some(handle)
}

/// The numeric `accountId` from the active character-status line, e.g.:
///
/// ```text
/// <…> <AccountLoginCharacterStatus_Character> Character: … - accountId 1155333 - name VeeLume - state STATE_CURRENT …
/// ```
///
/// Read only from the `STATE_CURRENT` character so defunct character
/// artifacts (the ephemeral `5xxxxxx` ids the identity investigation flagged)
/// don't leak in.
fn session_account_id(line: &str) -> Option<i64> {
    if !line.contains("<AccountLoginCharacterStatus_Character>") || !line.contains("STATE_CURRENT") {
        return None;
    }
    const PREFIX: &str = "accountId ";
    let start = line.find(PREFIX)? + PREFIX.len();
    let rest = &line[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // All fixtures below are verbatim lines from a live LIVE/Game.log
    // (build 4.x, 2026-05-30 session).

    #[test]
    fn parses_blueprint_received() {
        let line = r#"<2026-05-30T14:43:35.628Z> [Notice] <SHUDEvent_OnNotification> Added notification "Received Blueprint: Testudo Arms Clanguard: " [60] to queue. New queue size: 2, MissionId: [00000000-0000-0000-0000-000000000000], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]"#;
        assert_eq!(
            parse_line(line),
            Some(SensedEvent::BlueprintReceived { name: "Testudo Arms Clanguard".into() })
        );
    }

    #[test]
    fn parses_blueprint_with_alphanumeric_name() {
        let line = r#"<2026-05-30T14:43:35.628Z> [Notice] <SHUDEvent_OnNotification> Added notification "Received Blueprint: S3 Attrition-3 Repeater: " [61] to queue. New queue size: 3, MissionId: [00000000-0000-0000-0000-000000000000], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]"#;
        assert_eq!(
            parse_line(line),
            Some(SensedEvent::BlueprintReceived { name: "S3 Attrition-3 Repeater".into() })
        );
    }

    #[test]
    fn ignores_notification_lifecycle_lines() {
        // These repeat the blueprint text but are NOT new acquisitions.
        let next = r#"<2026-05-30T14:43:41.167Z> [Notice] <UpdateNotificationItem> Notification "Received Blueprint: Testudo Arms Clanguard: " [60], Action: Next [Team_CoreGameplayFeatures][Missions][Comms]"#;
        let fade = r#"<2026-05-30T14:43:46.170Z> [Notice] <UpdateNotificationItem> Notification "Received Blueprint: Testudo Arms Clanguard: " [60], Action: StartFade [Team_CoreGameplayFeatures][Missions][Comms]"#;
        let echo = r#"<2026-05-30T14:43:35.628Z>    "Received Blueprint: Testudo Arms Clanguard: " [60]"#;
        assert_eq!(parse_line(next), None);
        assert_eq!(parse_line(fade), None);
        assert_eq!(parse_line(echo), None);
    }

    #[test]
    fn parses_session_platform() {
        let line = r#"<2026-05-30T13:19:05.804Z>    [Cmdline* ] --envtag='PUB'"#;
        assert_eq!(parse_line(line), Some(SensedEvent::SessionPlatform(Platform::Prod)));
    }

    #[test]
    fn maps_non_pub_envtag_to_ptu() {
        let line = r#"<2026-05-30T13:19:05.804Z>    [Cmdline* ] --envtag='PTU'"#;
        assert_eq!(parse_line(line), Some(SensedEvent::SessionPlatform(Platform::Ptu)));
    }

    #[test]
    fn parses_session_handle() {
        let line = r#"<2026-05-30T13:19:29.286Z> [Notice] <Legacy login response> [CIG-net] User Login Success - Handle[VeeLume] - Time[341393589] [Team_GameServices][Login]"#;
        assert_eq!(parse_line(line), Some(SensedEvent::SessionHandle("VeeLume".into())));
    }

    #[test]
    fn parses_session_account_id() {
        let line = r#"<2026-05-30T13:19:26.325Z> [Notice] <AccountLoginCharacterStatus_Character> Character: createdAt 1778733647232 - updatedAt 1778788302608 - geid 204717100112 - accountId 1155333 - name VeeLume - state STATE_CURRENT [Team_GameServices][Login]"#;
        assert_eq!(parse_line(line), Some(SensedEvent::SessionAccountId(1155333)));
    }

    #[test]
    fn ignores_non_current_character_status() {
        let line = r#"<2026-05-30T13:19:26.325Z> [Notice] <AccountLoginCharacterStatus_Character> Character: … - accountId 5551234 - name OldChar - state STATE_DEFUNCT [Team_GameServices][Login]"#;
        assert_eq!(parse_line(line), None);
    }

    #[test]
    fn ignores_unrelated_lines() {
        let line = r#"<2026-05-30T13:20:14.660Z> [Notice] <StatObjLoad 0x800 Format> 'data/objectcontainers/pu/loc/…' - File exists in P4K [Team_Graphics][CoreTech]"#;
        assert_eq!(parse_line(line), None);
    }
}
