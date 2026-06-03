// Global notification store — the single frontend funnel.
//
// Every notification, whether it came from the backend `notify` event
// (src-tauri/src/notify.rs) or from in-app code, flows through `notify()` into
// one list. That list drives two surfaces: the transient toast stack
// (Toasts.svelte) and the persistent notification center (NotificationCenter
// .svelte). Session-memory only — cleared on restart, no DB (alpha decision).

import { listen, EVENTS, type UnlistenFn } from "$lib/ipc";

export type NotifLevel = "info" | "success" | "warning" | "error";

export type NotifAction = { label: string; href: string };

/** Shape emitted by the Rust `notify` helper; also the input to `notify()`. */
export type NotifPayload = {
  level: NotifLevel;
  title: string;
  body?: string | null;
  action?: NotifAction | null;
};

/** A notification once it's in the store (id/timestamp/read added here). */
export type Notification = NotifPayload & {
  id: string;
  ts: number;
  read: boolean;
};

/** Levels whose toasts persist until dismissed (vs auto-fading). */
const STICKY: Record<NotifLevel, boolean> = {
  info: false,
  success: false,
  warning: true,
  error: true,
};

/** Keep the in-memory log bounded so a long session can't grow without limit. */
const MAX_ITEMS = 100;

let items = $state<Notification[]>([]);
let counter = 0;

/** Reactive read access. Components use `notifications.items` / `.unread`. */
export const notifications = {
  get items() {
    return items;
  },
  get unread() {
    return items.reduce((acc, n) => acc + (n.read ? 0 : 1), 0);
  },
  get hasUnread() {
    return items.some((n) => !n.read);
  },
};

/** Add a notification. The one entry point — backend events and in-app code
 *  both call this. Returns the created notification. */
export function notify(input: NotifPayload): Notification {
  const n: Notification = {
    id: `n${counter++}`,
    ts: Date.now(),
    read: false,
    level: input.level,
    title: input.title,
    body: input.body ?? null,
    action: input.action ?? null,
  };
  items = [n, ...items].slice(0, MAX_ITEMS);
  return n;
}

export function dismiss(id: string) {
  items = items.filter((n) => n.id !== id);
}

export function markAllRead() {
  if (items.some((n) => !n.read)) {
    items = items.map((n) => (n.read ? n : { ...n, read: true }));
  }
}

export function clearAll() {
  items = [];
}

/** Whether a level's toast should stay until dismissed. */
export function isSticky(level: NotifLevel): boolean {
  return STICKY[level];
}

/** Subscribe to backend `notify` events. Call once, in the root layout. */
export function listenForNotifications(): Promise<UnlistenFn> {
  return listen<NotifPayload>(EVENTS.notify, (event) => {
    notify(event.payload);
  });
}
