// Persistent state + actions for the Game.log blueprint import.
//
// The scan reads hundreds of log files and can take a while. Keeping this state
// in a module (not the component) means it survives navigation: switch tabs or
// pages mid-scan and come back, and the UI still shows it running — and the
// result isn't lost when the scan finishes while the view is unmounted.

import {
  commands,
  type Account,
  type DiscoveredIdentity,
  type ImportChoice,
  type ImportResult,
} from "$lib/bindings";
import { refreshOwnership } from "$lib/data.svelte";

let _accounts = $state<Account[]>([]);
let _identities = $state<DiscoveredIdentity[] | null>(null);
let _scanning = $state(false);
let _importing = $state(false);
let _importResult = $state<ImportResult | null>(null);
let _error = $state<string | null>(null);
// key → select value: "__ignore__" | "__new__" | <account id>
let _choice = $state<Record<string, string>>({});

export const bpImport = {
  get accounts() {
    return _accounts;
  },
  get identities() {
    return _identities;
  },
  get scanning() {
    return _scanning;
  },
  get importing() {
    return _importing;
  },
  get importResult() {
    return _importResult;
  },
  get error() {
    return _error;
  },
  get choice() {
    return _choice;
  },
};

export function setChoice(key: string, value: string) {
  _choice[key] = value;
}

/** Refresh the account list (cheap DB read) — used for the mapping dropdown. */
export async function loadAccounts() {
  const res = await commands.listAccounts();
  if (res.status === "ok") _accounts = res.data;
}

export async function scan() {
  if (_scanning) return;
  _scanning = true;
  _error = null;
  _importResult = null;
  const res = await commands.scanLogHistory();
  if (res.status === "ok") {
    _identities = res.data;
    // Default each discovered identity to its suggested account, else a new one
    // (the user scanned in order to import); they can switch to ignore.
    const next: Record<string, string> = {};
    for (const id of res.data) next[id.key] = id.suggested_account_id ?? "__new__";
    _choice = next;
  } else {
    _error = `${res.error.kind}: ${res.error.message}`;
  }
  _scanning = false;
}

export async function applyImport() {
  if (!_identities || _importing) return;
  _importing = true;
  _error = null;
  const choices: ImportChoice[] = _identities.map((id) => {
    const val = _choice[id.key] ?? "__ignore__";
    if (val === "__ignore__") return { key: id.key, action: "ignore", account_id: null };
    if (val === "__new__") return { key: id.key, action: "new", account_id: null };
    return { key: id.key, action: "existing", account_id: val };
  });
  const res = await commands.applyLogImport(choices);
  if (res.status === "ok") {
    _importResult = res.data;
    _identities = null;
    // New accounts may have appeared; the imported BPs change the owned set —
    // refresh both so the dropdown and the catalog reflect the import.
    await loadAccounts();
    await refreshOwnership();
  } else {
    _error = `${res.error.kind}: ${res.error.message}`;
  }
  _importing = false;
}
