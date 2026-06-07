// Svelte action: remember a scroll container's position while the page
// component stays mounted, and restore it when the element re-mounts. Used for
// the in-route list ↔ detail toggle (`?bp=`): the list's scroll container is
// unmounted while the detail shows, so without this it returns to the top on
// Back. Each `persistentScroll()` call owns one closure-scoped position, so make
// one per list (it persists across the detail toggle, resets when the page
// itself unmounts on a real route change).

export function persistentScroll() {
  let top = 0;
  return (node: HTMLElement) => {
    // Restore on (re)mount. Content renders synchronously before the action
    // runs, so the scroll height is already correct for text lists.
    node.scrollTop = top;
    const onScroll = () => {
      top = node.scrollTop;
    };
    node.addEventListener("scroll", onScroll, { passive: true });
    return {
      destroy() {
        node.removeEventListener("scroll", onScroll);
      },
    };
  };
}
