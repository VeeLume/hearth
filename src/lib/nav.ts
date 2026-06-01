// Sidebar navigation definitions, shared by the layout. Kept in one
// place so adding a view is a single edit.

export type NavItem = {
  href: string;
  label: string;
  icon: string;
  /** Greyed out + non-navigable until the feature lands. */
  soon?: boolean;
};

export const primaryNav: NavItem[] = [
  { href: "/", label: "Catalog", icon: "▣" },
  { href: "/missions", label: "Missions", icon: "◆" },
  { href: "/wishlist", label: "Wishlist", icon: "♥" },
  { href: "/accounts", label: "Accounts", icon: "◉" },
];

export const futureNav: NavItem[] = [
  { href: "/crafting", label: "Crafting", icon: "⚒", soon: true },
  { href: "/community", label: "Community", icon: "❖", soon: true },
];
