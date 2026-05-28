<!--
  Throwaway layout mockup — NOT wired to real commands/state.
  Purpose: iterate on the v1 desktop shell (left sidebar rail + header +
  content) with fake data before porting the chosen direction into the
  real routes. Delete or fold into the real layout once settled.

  Visit at /mockup while running `pnpm tauri dev` (or in a browser —
  it's pure view layer, no Tauri calls).
-->
<script lang="ts">
  type Nav = { id: string; label: string; icon: string; soon?: boolean };

  const primaryNav: Nav[] = [
    { id: "catalog", label: "Catalog", icon: "▣" },
    { id: "missions", label: "Missions", icon: "◆" },
    { id: "wishlist", label: "Wishlist", icon: "★" },
  ];
  const futureNav: Nav[] = [
    { id: "crafting", label: "Crafting", icon: "⚒", soon: true },
    { id: "community", label: "Community", icon: "❖", soon: true },
  ];

  let active = $state("catalog");

  // owned = you own the BLUEPRINT (the recipe).
  // wantRecipe = you want to ACQUIRE the blueprint (mission-planning goal).
  //              Only meaningful while unowned — owning means you have it.
  // wantItem   = you want a CRAFTED COPY of the item in hand (crafting /
  //              community-craft-for-you goal). Independent of ownership.
  type Bp = {
    name: string;
    guid: string;
    owned: boolean;
    wantRecipe: boolean;
    wantItem: boolean;
  };
  type Pool = { name: string; bps: Bp[] };

  // Fake catalog so the layout has something realistic to frame.
  let pools = $state<Pool[]>([
    {
      name: "Ship Weapons",
      bps: [
        { name: "Attrition-3 Laser Repeater", guid: "a1b2c3", owned: true, wantRecipe: false, wantItem: true },
        { name: "CF-337 Panther Repeater", guid: "d4e5f6", owned: false, wantRecipe: true, wantItem: false },
        { name: "M5A Laser Cannon", guid: "079abc", owned: false, wantRecipe: false, wantItem: false },
        { name: "Scorpion GT-215 Gatling", guid: "112233", owned: true, wantRecipe: false, wantItem: false },
      ],
    },
    {
      name: "Mining Modules",
      bps: [
        { name: "Lifeline Medical Module", guid: "445566", owned: false, wantRecipe: false, wantItem: true },
        { name: "Hofstede S1 Mining Head", guid: "778899", owned: true, wantRecipe: false, wantItem: true },
        { name: "Helix I Mining Head", guid: "aabbcc", owned: false, wantRecipe: true, wantItem: true },
      ],
    },
    {
      name: "Personal Armor",
      bps: [
        { name: "Pembroke Heavy Helmet", guid: "ddeeff", owned: false, wantRecipe: false, wantItem: false },
        { name: "ORC-mkII Core", guid: "123456", owned: false, wantRecipe: true, wantItem: false },
      ],
    },
  ]);

  type Filter = "all" | "owned" | "unowned" | "wished";
  let filter = $state<Filter>("all");
  let query = $state("");

  const allBps = $derived(pools.flatMap((p) => p.bps));
  const ownedCount = $derived(allBps.filter((b) => b.owned).length);
  const recipeWishes = $derived(allBps.filter((b) => b.wantRecipe));
  const itemWishes = $derived(allBps.filter((b) => b.wantItem));
  // Sidebar badge: distinct BPs with any wish on them.
  const wishCount = $derived(
    allBps.filter((b) => b.wantRecipe || b.wantItem).length,
  );

  const visiblePools = $derived.by(() => {
    const q = query.toLowerCase().trim();
    return pools
      .map((p) => ({
        name: p.name,
        bps: p.bps.filter((b) => {
          if (filter === "owned" && !b.owned) return false;
          if (filter === "unowned" && b.owned) return false;
          if (filter === "wished" && !(b.wantRecipe || b.wantItem)) return false;
          if (q && !(b.name.toLowerCase().includes(q) || b.guid.includes(q)))
            return false;
          return true;
        }),
      }))
      .filter((p) => p.bps.length > 0);
  });

  const filters: { id: Filter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "owned", label: "Owned" },
    { id: "unowned", label: "Unowned" },
    { id: "wished", label: "Wished" },
  ];

  function activeLabel(id: string) {
    if (id === "settings") return "Settings";
    return (
      primaryNav.find((n) => n.id === id)?.label ??
      futureNav.find((n) => n.id === id)?.label ??
      ""
    );
  }
  function activeIcon(id: string) {
    if (id === "settings") return "⚙";
    return [...primaryNav, ...futureNav].find((n) => n.id === id)?.icon ?? "";
  }
</script>

<div class="app">
  <!-- ── Left rail ─────────────────────────────────────────── -->
  <aside class="sidebar">
    <div class="brand">
      <span class="flame">🔥</span>
      <span class="brand-name">Hearth</span>
    </div>

    <nav>
      {#each primaryNav as item (item.id)}
        <button
          class="nav-item"
          class:active={active === item.id}
          onclick={() => (active = item.id)}
        >
          <span class="nav-icon">{item.icon}</span>
          <span class="nav-label">{item.label}</span>
          {#if item.id === "wishlist" && wishCount}
            <span class="nav-badge">{wishCount}</span>
          {/if}
        </button>
      {/each}

      <div class="nav-divider"></div>

      {#each futureNav as item (item.id)}
        <button class="nav-item" disabled>
          <span class="nav-icon">{item.icon}</span>
          <span class="nav-label">{item.label}</span>
          <span class="soon">soon</span>
        </button>
      {/each}
    </nav>

    <!-- account / scope block pinned to the bottom -->
    <div class="account">
      <div class="account-handle">
        <span class="avatar">V</span>
        <div class="account-meta">
          <span class="handle">@VeeLume</span>
          <span class="scope-line">
            <span class="pf">PU</span>
            <span class="dot">·</span>
            <span class="chan">LIVE</span>
            <span class="verified" title="Verified Jan 31, 2016 · #1196670">✓</span>
          </span>
        </div>
        <button
          class="cog"
          class:active={active === "settings"}
          title="Settings"
          aria-label="Settings"
          onclick={() => (active = "settings")}
        >
          ⚙
        </button>
      </div>
    </div>
  </aside>

  <!-- ── Main column ───────────────────────────────────────── -->
  <main>
    <header class="topbar">
      <div class="page-title">
        <h1>{activeLabel(active)}</h1>
        <span class="subtitle">
          {allBps.length} blueprints · {ownedCount} owned
        </span>
      </div>
      <input
        class="search"
        type="search"
        placeholder="Search name or GUID…"
        bind:value={query}
      />
    </header>

    {#if active === "catalog"}
      <div class="filterbar">
        <div class="chips">
          {#each filters as f (f.id)}
            <button
              class="chip"
              class:on={filter === f.id}
              onclick={() => (filter = f.id)}
            >
              {f.label}
              {#if f.id === "owned"}<span class="chip-n">{ownedCount}</span>{/if}
              {#if f.id === "wished"}<span class="chip-n">{wishCount}</span>{/if}
            </button>
          {/each}
        </div>
        <div class="legend">
          <span class="legend-item"><span class="legend-icon own">✓</span> own BP</span>
          <span class="legend-item"><span class="legend-icon want">⚑</span> want BP</span>
          <span class="legend-item"><span class="legend-icon want">♥</span> want item</span>
        </div>
      </div>

      <section class="catalog">
        {#each visiblePools as pool (pool.name)}
          <div class="pool">
            <div class="pool-head">
              <span class="pool-name">{pool.name}</span>
              <span class="pool-count">{pool.bps.length}</span>
            </div>
            <ul>
              {#each pool.bps as bp (bp.guid)}
                <li class:owned={bp.owned}>
                  <button
                    class="own-toggle"
                    class:on={bp.owned}
                    title={bp.owned
                      ? "Blueprint owned — click to unmark"
                      : "Mark blueprint owned"}
                    onclick={() => (bp.owned = !bp.owned)}
                  >
                    {bp.owned ? "✓" : ""}
                  </button>
                  <span class="bp-name">{bp.name}</span>
                  <span class="bp-guid">{bp.guid}</span>

                  <div class="wish-group">
                    <!-- want the BLUEPRINT (recipe) — only while unowned -->
                    {#if !bp.owned}
                      <button
                        class="wish recipe"
                        class:on={bp.wantRecipe}
                        title={bp.wantRecipe
                          ? "Want this blueprint — remove"
                          : "Want this blueprint (recipe)"}
                        onclick={() => (bp.wantRecipe = !bp.wantRecipe)}
                      >
                        {bp.wantRecipe ? "⚑" : "⚐"}
                      </button>
                    {:else}
                      <span class="wish placeholder-slot" title="Blueprint owned"
                        >·</span
                      >
                    {/if}

                    <!-- want a CRAFTED ITEM — always available -->
                    <button
                      class="wish item"
                      class:on={bp.wantItem}
                      title={bp.wantItem
                        ? "Want a crafted copy — remove"
                        : "Want a crafted copy of the item"}
                      onclick={() => (bp.wantItem = !bp.wantItem)}
                    >
                      {bp.wantItem ? "♥" : "♡"}
                    </button>
                  </div>
                </li>
              {/each}
            </ul>
          </div>
        {/each}
        {#if visiblePools.length === 0}
          <p class="empty">Nothing matches this filter.</p>
        {/if}
      </section>
    {:else if active === "wishlist"}
      <section class="wishlist">
        <!-- Two intents, two sections, one underlying BP↔item identity. -->
        <div class="wish-section">
          <div class="wish-section-head">
            <span class="wish-section-icon want">⚑</span>
            <h2>Blueprints I want</h2>
            <span class="wish-section-count">{recipeWishes.length}</span>
          </div>
          <p class="wish-section-sub">
            Recipes to acquire. Fulfilled by mission rewards — the Missions
            view will highlight which missions drop these.
          </p>
          {#if recipeWishes.length === 0}
            <p class="empty">No blueprint wishes yet.</p>
          {:else}
            <ul class="wish-list">
              {#each recipeWishes as bp (bp.guid)}
                <li>
                  <span class="bp-name">{bp.name}</span>
                  <span class="status hunt">⛏ find via mission</span>
                  <button
                    class="wish recipe on"
                    title="Remove blueprint wish"
                    onclick={() => (bp.wantRecipe = false)}>⚑</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <div class="wish-section">
          <div class="wish-section-head">
            <span class="wish-section-icon want">♥</span>
            <h2>Items I want</h2>
            <span class="wish-section-count">{itemWishes.length}</span>
          </div>
          <p class="wish-section-sub">
            Crafted copies to obtain. Status reflects whether you can make it
            yet — crafting guidance fills in at v1.5; a friend crafting it for
            you arrives with the community layer (v2).
          </p>
          {#if itemWishes.length === 0}
            <p class="empty">No item wishes yet.</p>
          {:else}
            <ul class="wish-list">
              {#each itemWishes as bp (bp.guid)}
                <li>
                  <span class="bp-name">{bp.name}</span>
                  {#if bp.owned}
                    <span class="status ready">✓ recipe owned · craftable</span>
                  {:else}
                    <span class="status blocked">⚠ need blueprint first</span>
                  {/if}
                  <button
                    class="wish item on"
                    title="Remove item wish"
                    onclick={() => (bp.wantItem = false)}>♥</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
    {:else}
      <section class="placeholder">
        <span class="ph-icon">{activeIcon(active)}</span>
        <p>{activeLabel(active)} view — layout TBD.</p>
        <p class="ph-hint">
          This mockup focuses on the Catalog + Wishlist. The other views reuse
          the same shell (sidebar + topbar + content).
        </p>
      </section>
    {/if}
  </main>
</div>

<style>
  /* Mockup-local theme. Warm ember accent fitting "Hearth" — the real
     app currently uses a cool blue (#4d6cf3); this is a proposal. */
  .app {
    --bg: #131316;
    --panel: #1a1a1f;
    --panel-2: #1d1d22;
    --line: #2a2a32;
    --text: #e8e8ea;
    --muted: #8a8a95;
    --faint: #5a5a64;
    --ember: #e8915a;
    --ember-dim: #b86a3e;
    --ember-glow: rgba(232, 145, 90, 0.14);
    --good: #6cc08a;

    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100vh;
    background: var(--bg);
    color: var(--text);
    overflow: hidden;
  }

  /* ── Sidebar ── */
  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border-right: 1px solid var(--line);
    padding: 0.75rem 0.6rem;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.5rem 1rem;
  }
  .flame {
    font-size: 1.25rem;
    filter: saturate(1.1);
  }
  .brand-name {
    font-size: 1.15rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    flex: 1;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    padding: 0.5rem 0.65rem;
    background: transparent;
    border: none;
    border-radius: 7px;
    cursor: pointer;
    color: var(--muted);
    text-align: left;
    transition: background 90ms, color 90ms;
  }
  .nav-item:hover:not(:disabled) {
    background: var(--panel-2);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--ember-glow);
    color: var(--ember);
    font-weight: 500;
  }
  .nav-item:disabled {
    color: var(--faint);
    cursor: default;
  }
  .nav-icon {
    width: 1.1rem;
    text-align: center;
    font-size: 0.95rem;
  }
  .nav-label {
    flex: 1;
    font-size: 0.9rem;
  }
  .nav-badge {
    font-size: 0.7rem;
    background: var(--ember-dim);
    color: #fff;
    padding: 0.05rem 0.4rem;
    border-radius: 999px;
    font-variant-numeric: tabular-nums;
  }
  .soon {
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
    border: 1px solid var(--line);
    padding: 0.05rem 0.35rem;
    border-radius: 4px;
  }
  .nav-divider {
    height: 1px;
    background: var(--line);
    margin: 0.6rem 0.5rem;
  }

  .account {
    border-top: 1px solid var(--line);
    padding-top: 0.6rem;
    margin-top: 0.4rem;
  }
  .account-handle {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.35rem 0.5rem;
    border-radius: 7px;
  }
  .avatar {
    width: 1.9rem;
    height: 1.9rem;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--ember), var(--ember-dim));
    color: #1a1209;
    font-weight: 700;
    font-size: 0.85rem;
  }
  .account-meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .handle {
    font-size: 0.85rem;
    font-weight: 500;
  }
  .scope-line {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.7rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .pf {
    color: var(--ember);
    font-weight: 600;
  }
  .dot {
    color: var(--faint);
  }
  .verified {
    color: var(--good);
  }
  .cog {
    margin-left: auto;
    flex: 0 0 auto;
    width: 1.9rem;
    height: 1.9rem;
    display: grid;
    place-items: center;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: var(--muted);
    cursor: pointer;
    font-size: 1rem;
    transition: background 90ms, color 90ms;
  }
  .cog:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .cog.active {
    background: var(--ember-glow);
    color: var(--ember);
  }

  /* ── Main ── */
  main {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .topbar {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 1.1rem 1.6rem;
    border-bottom: 1px solid var(--line);
  }
  .page-title {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  h1 {
    margin: 0;
    font-size: 1.4rem;
    letter-spacing: -0.02em;
  }
  .subtitle {
    font-size: 0.78rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .search {
    margin-left: auto;
    width: 280px;
    padding: 0.5rem 0.8rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    outline: none;
    transition: border-color 90ms;
  }
  .search:focus {
    border-color: var(--ember);
  }

  .filterbar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.85rem 1.6rem;
  }
  .chips {
    display: flex;
    gap: 0.5rem;
  }
  .legend {
    margin-left: auto;
    display: flex;
    gap: 0.9rem;
    font-size: 0.72rem;
    color: var(--faint);
  }
  .legend-item {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .legend-icon {
    font-size: 0.85rem;
  }
  .legend-icon.own {
    color: var(--ember);
  }
  .legend-icon.want {
    color: var(--ember);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.32rem 0.75rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.on {
    background: var(--ember-glow);
    border-color: var(--ember-dim);
    color: var(--ember);
  }
  .chip-n {
    font-size: 0.68rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }

  .catalog {
    flex: 1;
    overflow-y: auto;
    padding: 0 1.6rem 2rem;
  }
  .pool {
    margin-bottom: 1.1rem;
  }
  .pool-head {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.4rem 0.2rem;
    position: sticky;
    top: 0;
    background: var(--bg);
  }
  .pool-name {
    font-size: 0.78rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .pool-count {
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  li:hover {
    background: var(--panel);
  }
  li.owned {
    background: linear-gradient(
      90deg,
      var(--ember-glow),
      transparent 60%
    );
  }
  .own-toggle {
    width: 1.4rem;
    height: 1.4rem;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1.5px solid var(--line);
    background: transparent;
    color: var(--bg);
    cursor: pointer;
    font-size: 0.8rem;
    transition: all 90ms;
  }
  .own-toggle:hover {
    border-color: var(--ember-dim);
  }
  .own-toggle.on {
    background: var(--ember);
    border-color: var(--ember);
    color: #1a1209;
    font-weight: 700;
  }
  .bp-name {
    flex: 1;
    font-size: 0.9rem;
  }
  .bp-guid {
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.72rem;
    color: var(--faint);
  }
  .wish-group {
    display: flex;
    align-items: center;
    gap: 0.1rem;
  }
  .wish {
    background: transparent;
    border: none;
    cursor: pointer;
    color: var(--faint);
    font-size: 1.05rem;
    line-height: 1;
    padding: 0.25rem 0.3rem;
    border-radius: 6px;
    transition: color 90ms, transform 90ms, background 90ms;
  }
  .wish:hover {
    transform: scale(1.15);
    color: var(--muted);
    background: var(--panel-2);
  }
  /* both intents glow ember when active (shared outline→fill language) */
  .wish.on {
    color: var(--ember);
  }
  .wish.placeholder-slot {
    color: var(--faint);
    opacity: 0.3;
    cursor: default;
    padding: 0.25rem 0.3rem;
  }
  .wish.placeholder-slot:hover {
    transform: none;
    background: transparent;
    color: var(--faint);
  }
  .empty {
    color: var(--muted);
    padding: 1.5rem 0.5rem;
  }

  /* ── Wishlist view ── */
  .wishlist {
    flex: 1;
    overflow-y: auto;
    padding: 1rem 1.6rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 1.75rem;
  }
  .wish-section-head {
    display: flex;
    align-items: center;
    gap: 0.55rem;
  }
  .wish-section-icon {
    font-size: 1.1rem;
    color: var(--ember);
  }
  .wish-section-head h2 {
    margin: 0;
    font-size: 1.05rem;
    font-weight: 600;
  }
  .wish-section-count {
    font-size: 0.75rem;
    color: var(--faint);
    background: var(--panel-2);
    border-radius: 999px;
    padding: 0.05rem 0.5rem;
    font-variant-numeric: tabular-nums;
  }
  .wish-section-sub {
    margin: 0.35rem 0 0.75rem;
    font-size: 0.8rem;
    color: var(--muted);
    max-width: 60ch;
  }
  .wish-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .wish-list li {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border-radius: 8px;
    background: var(--panel);
  }
  .status {
    font-size: 0.74rem;
    padding: 0.1rem 0.5rem;
    border-radius: 5px;
    white-space: nowrap;
  }
  .status.ready {
    color: var(--good);
    background: rgba(108, 192, 138, 0.1);
  }
  .status.blocked {
    color: #d9a441;
    background: rgba(217, 164, 65, 0.1);
  }
  .status.hunt {
    color: var(--muted);
    background: var(--panel-2);
  }

  .placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    color: var(--muted);
  }
  .ph-icon {
    font-size: 2.5rem;
    opacity: 0.5;
  }
  .ph-hint {
    font-size: 0.8rem;
    color: var(--faint);
    max-width: 320px;
    text-align: center;
  }
</style>
