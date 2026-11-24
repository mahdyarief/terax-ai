// Contract: every menu popover takes its color/duration tokens from src/styles/tokens.ts;
// wrappers compose these constants and must not redefine radius or shadow locally.
export const menuContentClass =
  "rounded-3xl bg-popover p-1.5 text-popover-foreground shadow-lg ring-1 ring-foreground/5 duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2";

export const menuContentMotionClass =
  "dark:ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

export const menuItemClass =
  "relative flex cursor-default items-center gap-2.5 rounded-2xl px-3 py-2 text-sm font-medium outline-hidden select-none focus:bg-accent focus:text-accent-foreground";

export const menuItemStateClass =
  "data-inset:pl-9.5 data-[variant=destructive]:text-destructive data-[variant=destructive]:focus:bg-destructive/10 data-[variant=destructive]:focus:text-destructive dark:data-[variant=destructive]:focus:bg-destructive/20 data-disabled:pointer-events-none data-disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4";

// Destructive-variant icon tint, shared by both wrappers' items.
export const menuItemDestructiveIconClass =
  "data-[variant=destructive]:*:[svg]:text-destructive";

export const menuSeparatorClass = "-mx-1.5 my-1.5 h-px bg-border/50";

export const menuLabelClass =
  "px-3 py-2.5 text-xs text-muted-foreground data-inset:pl-9.5";

export const menuShortcutClass =
  "ml-auto text-xs tracking-widest text-muted-foreground";

// Menus opened from the status bar: rows, labels and icons the size of its
// own text (11 px, icons 12 px). Put it on the content (and a sub-content,
// which is portalled); an item sizes an icon without `size-*` to 16 px.
export const statusMenuClass =
  "[&_[data-slot^=dropdown-menu-]]:gap-1.5 [&_[data-slot^=dropdown-menu-]]:text-[11px] [&_svg]:size-3";

// A plain status bar menu that can grow long (folders, hosts) scrolls, with a
// visible scrollbar. Not for menus that scroll a list of their own.
export const statusMenuScrollClass =
  "terax-scrollbar max-h-80 overflow-y-auto";
