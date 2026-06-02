// First-launch onboarding visibility.
//
// The overlay is rendered by the root layout when `onboarding.open` is true.
// On app start we open it once if the `onboarding_completed` setting is false;
// it can also be re-opened from Settings. Finishing or skipping marks the
// setting so it doesn't reappear on the next launch.

import { commands } from "$lib/bindings";

let _open = $state(false);
let _checked = false;

export const onboarding = {
  get open() {
    return _open;
  },
};

/** On app start, open onboarding if it hasn't been completed. Runs once. */
export async function maybeStart() {
  if (_checked) return;
  _checked = true;
  const r = await commands.getSettings();
  if (r.status === "ok" && !r.data.onboarding_completed) _open = true;
}

/** Re-open onboarding (e.g. from Settings). */
export function openOnboarding() {
  _open = true;
}

/** Mark completed + close. Used by both Finish and Skip. */
export async function finishOnboarding() {
  await commands.setOnboardingComplete();
  _open = false;
}
