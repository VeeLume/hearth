// One-click blueprint import from the local game logs.
//
// Scans the live Game.log + logbackups/ for "received blueprint" lines,
// auto-maps each discovered RSI identity to its suggested account (else a new
// one), marks the blueprints owned, and reports the outcome via a
// notification. The single import action — used by both Settings and
// onboarding. State lives here (not the component) so it survives navigation
// and finishes in the background. Corrections to a mis-mapped identity are made
// in Settings → Account (delete/merge — the import is idempotent).

import { commands, errText, type ImportChoice, type ImportResult } from "$lib/ipc";
import { refreshOwnership } from "$lib/data.svelte";
import { notify } from "$lib/notifications.svelte";

let _running = $state(false);
let _result = $state<ImportResult | null>(null);
let _error = $state<string | null>(null);

export const bpImport = {
  get running() {
    return _running;
  },
  get result() {
    return _result;
  },
  get error() {
    return _error;
  },
};

/** `quiet`: only notify if something was actually imported (or an error) —
 *  for the silent startup catch-up. `createNew`: create accounts for
 *  unrecognised identities (true = user-initiated import; false = startup
 *  catch-up, which only touches accounts that already exist). */
export async function runImport(opts: { quiet?: boolean; createNew?: boolean } = {}) {
  const quiet = opts.quiet ?? false;
  const createNew = opts.createNew ?? true;
  if (_running) return;
  _running = true;
  _error = null;
  _result = null;
  try {
    const scanRes = await commands.scanLogHistory();
    if (scanRes.status !== "ok") {
      _error = errText(scanRes.error);
      notify({ level: "error", title: "Log import failed", body: _error });
      return;
    }
    const ids = scanRes.data;
    if (ids.length === 0) {
      if (!quiet) {
        notify({
          level: "info",
          title: "Nothing to import",
          body: "No blueprint history found in your game logs.",
        });
      }
      return;
    }
    // Auto-map: each discovered identity → its suggested account. Unrecognised
    // identities become a new account when `createNew`, else they're ignored
    // (mis-maps are fixed in Settings → Account).
    const choices: ImportChoice[] = ids.map((id) =>
      id.suggested_account_id
        ? { key: id.key, action: "existing", account_id: id.suggested_account_id }
        : { key: id.key, action: createNew ? "new" : "ignore", account_id: null },
    );
    const res = await commands.applyLogImport(choices);
    if (res.status === "ok") {
      const r = res.data;
      _result = r;
      await refreshOwnership();
      if (!(quiet && r.newly_owned === 0)) {
        const multi = r.accounts_touched > 1;
        const plural = r.newly_owned === 1 ? "" : "s";
        notify({
          level: "success",
          title: multi
            ? `Imported ${r.newly_owned} blueprint${plural} across ${r.accounts_touched} accounts`
            : `Imported ${r.newly_owned} blueprint${plural} from your logs`,
          body: multi
            ? "More than one account turned up — review or merge them in Settings → Account."
            : r.unresolved.length
              ? `${r.unresolved.length} not recognised in the catalog`
              : null,
          action: multi
            ? { label: "Review accounts", href: "/settings" }
            : { label: "View catalog", href: "/" },
        });
      }
    } else {
      _error = errText(res.error);
      notify({ level: "error", title: "Log import failed", body: _error });
    }
  } finally {
    _running = false;
  }
}

/** Startup catch-up: when live game-log sensing is on (and onboarding is done),
 *  quietly re-import so blueprints received while the app was closed — now in
 *  `logbackups/` — get picked up. Cheap thanks to the per-file scan cache. */
export async function maybeStartupImport() {
  const r = await commands.getSettings();
  if (r.status === "ok" && r.data.sensor_enabled && r.data.onboarding_completed) {
    runImport({ quiet: true, createNew: false });
  }
}
