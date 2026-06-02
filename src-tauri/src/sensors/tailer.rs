//! Reading `Game.log` into [`SensedEvent`]s — whole-file summary, one-shot
//! scan, and the incremental poll-based tailer.
//!
//! The format-fragile per-line recognition lives in [`super::parse`]; this
//! module is the I/O layer that feeds bytes through it. Two read shapes:
//!
//! - [`summarize_session`] / [`scan_reader`] — whole-reader passes (a rotated
//!   backup is one finished session, so there's no offset bookkeeping).
//! - [`GameLogTailer`] — an incremental reader holding a byte offset; each
//!   [`GameLogTailer::poll`] returns the events from bytes appended since the
//!   last poll. A trailing partial line is carried until its newline arrives.
//!
//! The first poll starts at offset 0, so it scans the whole existing log:
//! that backfills blueprints already received this session **and** captures
//! the session header (env + handle) the pollution guard needs, before
//! switching to incremental tailing.

use std::collections::HashSet;
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::PathBuf;

use hearth_core::Platform;

use super::SensedEvent;
use super::parse;

/// What one session log (the live `Game.log` or a rotated backup) tells us:
/// who played, on what platform, and which blueprints they received. The
/// history import folds these across all backups, grouping by `account_hint`
/// (falling back to handle) to surface the identities the user then confirms.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    pub platform: Option<Platform>,
    pub handle: Option<String>,
    /// Numeric CIG `accountId` for the session, if present.
    pub account_hint: Option<i64>,
    /// Distinct blueprint display names received this session (in first-seen
    /// order). Display names — the importer resolves them to guids via the
    /// catalog.
    pub blueprint_names: Vec<String>,
}

/// Summarise a single session log. Whole-file read (a backup is one session),
/// so unlike the live tailer there's no offset bookkeeping.
pub fn summarize_session<R: BufRead>(reader: R) -> SessionSummary {
    let mut summary = SessionSummary::default();
    let mut seen = HashSet::new();
    // split(b'\n') + lossy decode, NOT lines(): Game.log has stray non-UTF-8
    // bytes, and lines().map_while(Result::ok) would truncate the scan at the
    // first one — dropping every receipt below it (the real cause of import
    // misses; blueprint lines sit deep in the file, well after such bytes).
    for bytes in reader.split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&bytes);
        match parse::parse_line(&line) {
            Some(SensedEvent::SessionPlatform(p)) => summary.platform = Some(p),
            Some(SensedEvent::SessionHandle(h)) => summary.handle = Some(h),
            Some(SensedEvent::SessionAccountId(id)) => summary.account_hint = Some(id),
            Some(SensedEvent::BlueprintReceived { name }) => {
                if seen.insert(name.clone()) {
                    summary.blueprint_names.push(name);
                }
            }
            None => {}
        }
    }
    summary
}

/// Parse every line of a reader into the events Hearth tracks, in order.
///
/// Splits on raw `\n` and lossy-decodes each line, NOT `lines()` — Game.log
/// contains stray non-UTF-8 bytes (Latin-1 chars like `ü`, occasional binary),
/// and `lines().map_while(Result::ok)` would halt the whole iterator at the
/// first such line, silently dropping every later receipt. `split(b'\n')` only
/// errors on real I/O faults, so a bad byte just garbles one line.
pub fn scan_reader<R: BufRead>(reader: R) -> Vec<SensedEvent> {
    reader
        .split(b'\n')
        .map_while(Result::ok)
        .filter_map(|bytes| {
            let line = String::from_utf8_lossy(&bytes);
            parse::parse_line(&line)
        })
        .collect()
}

/// Incremental `Game.log` reader. Holds a byte offset and the trailing
/// partial line, so repeated [`poll`](Self::poll)s see each appended line
/// exactly once.
///
/// Poll-based rather than `notify`-based: a file being appended by another
/// process is exactly the case filesystem-notification APIs handle least
/// reliably on Windows, and a periodic read is simple and robust.
#[derive(Debug)]
pub struct GameLogTailer {
    path: PathBuf,
    /// Bytes consumed so far.
    offset: u64,
    /// Carried partial line — bytes after the last newline of the previous
    /// poll, awaiting the rest of the line.
    pending: String,
}

impl GameLogTailer {
    /// Start at the beginning of the file, so the first [`poll`](Self::poll)
    /// scans the whole current log (backfill + session header).
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            pending: String::new(),
        }
    }

    /// Read bytes appended since the last poll and return the events among
    /// the now-complete lines. Returns an empty vec when nothing was appended
    /// (or the file doesn't exist yet). A shrunk file (new session wrote a
    /// fresh `Game.log`) resets the tailer to the start.
    pub fn poll(&mut self) -> std::io::Result<Vec<SensedEvent>> {
        let mut file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let len = file.metadata()?.len();
        if len < self.offset {
            // Rotated / truncated — restart from the top of the new log.
            self.offset = 0;
            self.pending.clear();
        }
        if len == self.offset {
            return Ok(Vec::new());
        }

        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::with_capacity((len - self.offset) as usize);
        file.take(len - self.offset).read_to_end(&mut buf)?;
        self.offset = len;

        // Game.log is UTF-8; lossy-decode guards against a torn multibyte
        // char at the read boundary — `pending` carries the text forward so
        // the line completes on a later poll.
        self.pending.push_str(&String::from_utf8_lossy(&buf));

        let mut events = Vec::new();
        while let Some(nl) = self.pending.find('\n') {
            let line = self.pending[..nl].trim_end_matches('\r').to_owned();
            if let Some(ev) = parse::parse_line(&line) {
                events.push(ev);
            }
            self.pending.drain(..=nl);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hearth_core::Platform;
    use std::io::Cursor;

    /// A realistic session slice: header (env + login), two blueprint grants,
    /// and the lifecycle/echo noise that must not double-count.
    const SAMPLE: &str = concat!(
        "<2026-05-30T13:19:05.804Z>    [Cmdline* ] --envtag='PUB'\n",
        "<2026-05-30T13:19:29.286Z> [Notice] <Legacy login response> [CIG-net] User Login Success - Handle[VeeLume] - Time[341393589] [Team_GameServices][Login]\n",
        "<2026-05-30T13:20:14.660Z> [Notice] <StatObjLoad 0x800 Format> 'data/...' - File exists in P4K [Team_Graphics][CoreTech]\n",
        "<2026-05-30T14:43:35.628Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Testudo Arms Clanguard: \" [60] to queue. New queue size: 2, MissionId: [00000000-0000-0000-0000-000000000000], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]\n",
        "<2026-05-30T14:43:41.167Z> [Notice] <UpdateNotificationItem> Notification \"Received Blueprint: Testudo Arms Clanguard: \" [60], Action: Next [Team_CoreGameplayFeatures][Missions][Comms]\n",
        "<2026-05-30T14:43:35.628Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: S3 Attrition-3 Repeater: \" [61] to queue. New queue size: 3, MissionId: [00000000-0000-0000-0000-000000000000], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]\n",
    );

    #[test]
    fn summarize_session_captures_identity_and_distinct_blueprints() {
        let log = concat!(
            "<…>    [Cmdline* ] --envtag='PUB'\n",
            "<…> [Notice] <AccountLoginCharacterStatus_Character> Character: - accountId 1155333 - name VeeLume - state STATE_CURRENT [Team_GameServices][Login]\n",
            "<…> [Notice] <Legacy login response> [CIG-net] User Login Success - Handle[VeeLume] - Time[1] [Team_GameServices][Login]\n",
            "<…> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo: \" [1] to queue. [Team]\n",
            "<…> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo: \" [2] to queue. [Team]\n",
            "<…> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Bar: \" [3] to queue. [Team]\n",
        );
        let s = summarize_session(Cursor::new(log));
        assert_eq!(s.platform, Some(Platform::Prod));
        assert_eq!(s.handle.as_deref(), Some("VeeLume"));
        assert_eq!(s.account_hint, Some(1155333));
        // "Foo" received twice → recorded once; order preserved.
        assert_eq!(
            s.blueprint_names,
            vec!["Foo".to_string(), "Bar".to_string()]
        );
    }

    #[test]
    fn summarize_survives_non_utf8_lines() {
        // A stray non-UTF-8 byte (Latin-1 'ü' = 0xFC) on a line BEFORE the
        // receipt must NOT truncate the scan — the regression that silently
        // dropped deep blueprint receipts across many backups.
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"<x> [Cmdline* ] --envtag='PUB'\n");
        bytes.extend_from_slice(b"<x> [Notice] <Legacy login response> Handle[VeeLume] [Login]\n");
        bytes.extend_from_slice(b"<x> chat: Regel ist ung\xFCltig\n"); // invalid UTF-8
        bytes.extend_from_slice(
            b"<x> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo Bar: \" [1] to queue. [Team]\n",
        );
        let s = summarize_session(Cursor::new(bytes));
        assert_eq!(s.platform, Some(Platform::Prod));
        assert_eq!(s.handle.as_deref(), Some("VeeLume"));
        assert_eq!(
            s.blueprint_names,
            vec!["Foo Bar".to_string()],
            "receipt after a non-UTF-8 line must still be captured"
        );
    }

    #[test]
    fn scan_reader_extracts_in_order_without_double_counting() {
        let events = scan_reader(Cursor::new(SAMPLE));
        assert_eq!(
            events,
            vec![
                SensedEvent::SessionPlatform(Platform::Prod),
                SensedEvent::SessionHandle("VeeLume".into()),
                SensedEvent::BlueprintReceived {
                    name: "Testudo Arms Clanguard".into()
                },
                SensedEvent::BlueprintReceived {
                    name: "S3 Attrition-3 Repeater".into()
                },
            ]
        );
    }

    #[test]
    fn tailer_reads_appended_lines_once() {
        use std::io::Write;

        let path = std::env::temp_dir().join("hearth_tailer_test_game.log");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, SAMPLE).unwrap();

        let mut tailer = GameLogTailer::new(path.clone());
        let first = tailer.poll().unwrap();
        assert_eq!(first.len(), 4, "first poll backfills the whole file");

        // Nothing new yet.
        assert!(tailer.poll().unwrap().is_empty());

        // Append one complete blueprint grant + a partial line (no newline).
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(
            f,
            "<2026-05-30T15:00:00.000Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo Bar: \" [62] to queue. MissionId: [x] [Team]\n<2026-05-30T15:00:01.000Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Partial"
        )
        .unwrap();
        f.flush().unwrap();

        let second = tailer.poll().unwrap();
        assert_eq!(
            second,
            vec![SensedEvent::BlueprintReceived {
                name: "Foo Bar".into()
            }],
            "complete line parsed; the partial trailing line is held back"
        );

        // Complete the partial line.
        writeln!(f, " Two: \" [63] to queue. MissionId: [x] [Team]").unwrap();
        f.flush().unwrap();
        let third = tailer.poll().unwrap();
        assert_eq!(
            third,
            vec![SensedEvent::BlueprintReceived {
                name: "Partial Two".into()
            }],
            "the carried partial completes on the next poll"
        );

        let _ = std::fs::remove_file(&path);
    }
}
