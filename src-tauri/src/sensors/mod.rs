//! v1.5 — Game.log tailing and automatic SC-state sensing.
//!
//! Kept in its own module from Stage 0 so when the log format inevitably
//! churns with a new SC patch, the blast radius is local and replaceable.
//!
//! # Shape
//!
//! - [`parse`] — pure, per-line recognisers (the format-fragile core, fully
//!   unit-tested against real log samples).
//! - [`scan_reader`] — parse a whole reader at once. Used for a one-shot
//!   backfill of the current session's log and in tests.
//! - [`GameLogTailer`] — an incremental reader holding a byte offset; each
//!   [`GameLogTailer::poll`] returns the events from bytes appended since the
//!   last poll. A trailing partial line is carried until its newline arrives.
//!
//! The first poll starts at offset 0, so it scans the whole existing log:
//! that backfills blueprints already received this session **and** captures
//! the session header (env + handle) the pollution guard needs, before
//! switching to incremental tailing.
//!
//! Wiring into `AppState` (resolve blueprint name → guid via the catalog,
//! pollution-guard the session against the active account + platform, then
//! mark owned) lives with the commands — this module only turns log bytes
//! into [`SensedEvent`]s.

pub mod parse;

pub use parse::SensedEvent;

use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The conventional `Game.log` path inside an install's channel directory
/// (e.g. `…/StarCitizen/LIVE/Game.log`).
pub fn game_log_path(channel_dir: &Path) -> PathBuf {
    channel_dir.join("Game.log")
}

/// Parse every line of a reader into the events Hearth tracks, in order.
/// Lines that fail to read (mid-rotation truncation, etc.) are skipped.
pub fn scan_reader<R: BufRead>(reader: R) -> Vec<SensedEvent> {
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| parse::parse_line(&line))
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
        Self { path, offset: 0, pending: String::new() }
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
    fn scan_reader_extracts_in_order_without_double_counting() {
        let events = scan_reader(Cursor::new(SAMPLE));
        assert_eq!(
            events,
            vec![
                SensedEvent::SessionPlatform(Platform::Prod),
                SensedEvent::SessionHandle("VeeLume".into()),
                SensedEvent::BlueprintReceived { name: "Testudo Arms Clanguard".into() },
                SensedEvent::BlueprintReceived { name: "S3 Attrition-3 Repeater".into() },
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
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(
            f,
            "{}",
            "<2026-05-30T15:00:00.000Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo Bar: \" [62] to queue. MissionId: [x] [Team]\n<2026-05-30T15:00:01.000Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Partial"
        )
        .unwrap();
        f.flush().unwrap();

        let second = tailer.poll().unwrap();
        assert_eq!(
            second,
            vec![SensedEvent::BlueprintReceived { name: "Foo Bar".into() }],
            "complete line parsed; the partial trailing line is held back"
        );

        // Complete the partial line.
        write!(f, "{}", " Two: \" [63] to queue. MissionId: [x] [Team]\n").unwrap();
        f.flush().unwrap();
        let third = tailer.poll().unwrap();
        assert_eq!(
            third,
            vec![SensedEvent::BlueprintReceived { name: "Partial Two".into() }],
            "the carried partial completes on the next poll"
        );

        let _ = std::fs::remove_file(&path);
    }
}
