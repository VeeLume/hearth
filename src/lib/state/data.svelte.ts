// Navigation-persistent shared data layer.
//
// Each page used to fetch its own copy of the catalog / missions / ownership on
// mount and throw it away on navigation, so every page switch re-fetched and
// flashed a loading screen — even though the backend already had the data warm.
//
// This module holds that data once, in memory, for the life of the app session.
// Pages read from it and mutate the shared sets, so switching pages is instant
// and a toggle on one page is reflected everywhere without a refetch.
//
// `ensureX()` loads on first call and is a no-op afterwards (it returns the
// in-flight promise to concurrent callers, and resets on failure so a later
// visit can retry). Each resolves to an error message string, or null on
// success.

import { SvelteSet } from "svelte/reactivity";
import {
  commands,
  errText,
  onOwnershipChanged,
  type UnlistenFn,
  type BpView,
  type MissionView,
  type MissionRef,
  type WishIntent,
} from "$lib/ipc";

// Ownership sets — shared so a toggle on any page updates them everywhere.
export const owned = new SvelteSet<string>();
export const wishRecipe = new SvelteSet<string>();
export const wishItem = new SvelteSet<string>();

export function wishSet(intent: WishIntent): SvelteSet<string> {
  return intent === "recipe" ? wishRecipe : wishItem;
}

let _blueprints = $state<BpView[]>([]);
let _missions = $state<MissionView[]>([]);
let _grantedBy = $state<Partial<Record<string, MissionRef[]>>>({});
let _blueprintsReady = $state(false);
let _missionsReady = $state(false);
let _ownershipReady = $state(false);
let _grantedByReady = $state(false);

/** Reactive read access to the shared, navigation-persistent data. */
export const data = {
  get blueprints() {
    return _blueprints;
  },
  get missions() {
    return _missions;
  },
  get grantedBy() {
    return _grantedBy;
  },
  get blueprintsReady() {
    return _blueprintsReady;
  },
  get missionsReady() {
    return _missionsReady;
  },
  get ownershipReady() {
    return _ownershipReady;
  },
  get grantedByReady() {
    return _grantedByReady;
  },
};

let bpPromise: Promise<string | null> | null = null;
let mPromise: Promise<string | null> | null = null;
let ownPromise: Promise<string | null> | null = null;
let gbPromise: Promise<string | null> | null = null;

export function ensureBlueprints(): Promise<string | null> {
  if (_blueprintsReady) return Promise.resolve(null);
  if (!bpPromise) {
    bpPromise = (async () => {
      const r = await commands.listBlueprints();
      if (r.status === "ok") {
        _blueprints = r.data;
        _blueprintsReady = true;
        return null;
      }
      bpPromise = null; // allow a retry on a later visit
      return errText(r.error);
    })();
  }
  return bpPromise;
}

export function ensureMissions(): Promise<string | null> {
  if (_missionsReady) return Promise.resolve(null);
  if (!mPromise) {
    mPromise = (async () => {
      const r = await commands.listMissions();
      if (r.status === "ok") {
        _missions = r.data;
        _missionsReady = true;
        return null;
      }
      mPromise = null;
      return errText(r.error);
    })();
  }
  return mPromise;
}

export function ensureGrantedBy(): Promise<string | null> {
  if (_grantedByReady) return Promise.resolve(null);
  if (!gbPromise) {
    gbPromise = (async () => {
      const r = await commands.missionsByBlueprint();
      if (r.status === "ok") {
        _grantedBy = r.data;
        _grantedByReady = true;
        return null;
      }
      gbPromise = null;
      return errText(r.error);
    })();
  }
  return gbPromise;
}

function applyOwnership(
  o: { blueprint_guid: string }[],
  w: { blueprint_guid: string; intent: WishIntent }[],
) {
  owned.clear();
  for (const x of o) owned.add(x.blueprint_guid);
  wishRecipe.clear();
  wishItem.clear();
  for (const x of w) wishSet(x.intent).add(x.blueprint_guid);
}

export function ensureOwnership(): Promise<string | null> {
  if (_ownershipReady) return Promise.resolve(null);
  if (!ownPromise) {
    ownPromise = (async () => {
      const [o, w] = await Promise.all([commands.listOwned(), commands.listWishlist()]);
      if (o.status === "ok" && w.status === "ok") {
        applyOwnership(o.data, w.data);
        _ownershipReady = true;
        return null;
      }
      ownPromise = null;
      const err = o.status === "error" ? o.error : w.status === "error" ? w.error : null;
      return err ? errText(err) : "failed to load ownership";
    })();
  }
  return ownPromise;
}

/** Re-pull owned + wishlist from the DB (e.g. after a live sync reconciles
 *  ownership behind the UI's back). Updates the shared sets in place. */
export async function refreshOwnership() {
  const [o, w] = await Promise.all([commands.listOwned(), commands.listWishlist()]);
  if (o.status === "ok" && w.status === "ok") applyOwnership(o.data, w.data);
}

/** Subscribe to the backend's `ownership-changed` event (live sync reconcile,
 *  sensor auto-mark) and refresh the shared owned set so the catalog count
 *  stays current without a restart. Call once, in the root layout. */
export function listenForOwnershipChanges(): Promise<UnlistenFn> {
  return onOwnershipChanged(refreshOwnership);
}

// ── Mutations ─────────────────────────────────────────────────────────────
// Optimistic single-record toggles, shared by every page that writes ownership
// or wishlist state (catalog, missions, wishlist). Each flips the shared set
// immediately, then reconciles with the backend's returned truth and reverts on
// error. They resolve to an error string on failure (caller surfaces it) or
// null on success — the pages used to each carry their own copy of this.

/** Flip ownership of one blueprint record. Marking it owned also clears any
 *  want-blueprint wish locally, mirroring what `add_owned` does server-side so
 *  the shared sets stay consistent without a refetch. */
export async function toggleOwned(guid: string): Promise<string | null> {
  const wasOwned = owned.has(guid);
  if (wasOwned) owned.delete(guid);
  else owned.add(guid);

  const result = await commands.toggleOwned(guid);
  if (result.status === "ok") {
    if (result.data) {
      owned.add(guid);
      wishRecipe.delete(guid);
    } else {
      owned.delete(guid);
    }
    return null;
  }
  // Revert the optimistic flip.
  if (wasOwned) owned.add(guid);
  else owned.delete(guid);
  return errText(result.error);
}

/** Flip one wishlist intent for one blueprint record. */
export async function toggleWishlist(guid: string, intent: WishIntent): Promise<string | null> {
  const set = wishSet(intent);
  const wasWanted = set.has(guid);
  if (wasWanted) set.delete(guid);
  else set.add(guid);

  const result = await commands.toggleWishlist(guid, intent);
  if (result.status === "ok") {
    if (result.data) set.add(guid);
    else set.delete(guid);
    return null;
  }
  if (wasWanted) set.add(guid);
  else set.delete(guid);
  return errText(result.error);
}
