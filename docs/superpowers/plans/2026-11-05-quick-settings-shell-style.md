# Quick settings in the shell's own style

The owner, 2026-11-05: the quick settings card "feels a bit laggy compared
to the rest of the shell, and its style must be updated to match the rest
of the shell. Functionality wise it seems good."

What was measured: no panel frame on g815 takes 4ms or more to build and
draw (a `slow panel` trace in `surfaces/panel/host.rs`, mirroring the
launcher's `slow launcher` line, printed nothing over a whole
`--quick-settings` run), so the lag is the card's motion, not its cost:

- an in-place menu grows on `spatialFast` (350, overshooting), and only
  once it has landed do its rows fade in on `effects` (200): about 550ms
  before a row shows or takes a key. Every other disclosure in the shell
  (the network panel's Known networks, the launcher's levels) shows its
  rows at once and lets the card morph.
- the dim rises on `effectsSlow` (300).
- a toggle flipping crossfades its whole 178x48 pill to `primary` on
  `Cx::color` (300), where every other surface's on state is a 32px chip.

What was decided: quick settings keeps its function and its order (the
system row, the sliders, the grid, Background, the menus, the keyboard,
the IPC) and drops GNOME's material and numbers for the panel language
(`docs/superpowers/specs/2026-10-30-panel-language.md`) and the shell's
own boxes. The spec wins: Task 2 rewrites its §2 first.

## Task 1: menu motion like the rest of the shell

Files: `surfaces/panel/quick/mod.rs` (`MenuState`), `surfaces/panel/host.rs`
(the `slow panel` trace, already in the tree uncommitted: commit it here),
`dev/smoke.d/quick_settings.sh`, spec §2 Motion and §4.

- Opening: the height grows on `spatialFast` and the rows fade in on
  `effectsFast` from the same instant; the rows are stops (`ready`) and the
  cursor moves into the menu from the first frame, not once it lands.
- Closing: the rows fade out on `effectsFast` and the height shrinks on
  `spatialFast`, both from the same instant.
- The dim rises and falls on `effects`.
- The leg's burst (the Volume arrow at a tenth speed) asserts the new
  shape: some frame with the height part way (0 < h < 0.98) and the
  content already past 0, no frame with content at 0 once the height is
  past 0.5, the rows under it moving down, the dim up. Delete the old
  "rows only once its height lands" claims.
- Verify: `just smoke --quick-settings` and `just smoke --quick-settings
  --light` green on g815, the burst frames read. Commit
  `fix(quick-settings): menu rows arrive with the menu, not after it`.

## Task 2: the shell's chrome on the grid

Files: `crates/fs-theme/src/tokens.rs`, `crates/fs-theme/src/style.rs`,
`crates/fs-theme/formalshell.json`, `formalshell-rs/src/ui/{w,draw,el,boxes}.rs`,
`surfaces/panel/quick/mod.rs`, the three quick settings legs, the spec
(§2, §3, §4, §5, §6, §13), `docs/DESIGN.md`'s quick settings paragraph.

- Delete the `quickInset`, `quickGap`, `quickToggle`, `quickArrow`,
  `quickButton`, `quickSlider` and `quickBackground` tokens, the
  `quick.toggle` and `quick.arrow` roles, and the `quick` and `arrow` cell
  states with their drawing branches (`stop_radius`, `cell`, the arrow
  `segment` in `boxes::paint_ends`). Whatever only they used goes too.
- The card is a headerless panel like any other: the host's own frame,
  the body at `panelRowInset`, no hand-made padding; `panelWidthWide`
  across; rows and grid columns `panelGap` apart.
- A toggle is the Settings page's tile: a `panel.group` cell (interactive,
  so its border brightens and the kit's wash eases in under the pointer),
  `panelRow` tall, holding a `panel_chip` with its glyph (lit `primary`
  while on) and the title in `body` 500 over its subtitle in `caption`
  `mutedForeground`, as `panel_row` lays out a row. While on, the tile is
  the group's `open` state too. Disabled stays the kit's 0.5 fade with no
  stop.
- A split toggle is two group tiles side by side, `rowGap` apart: the
  toggle (`id`) and a `panelRow` square arrow tile (`<id>:menu`) holding
  `chevron-right`, both lit while on.
- An info tile (§7) is a toggle that is never on.
- The system row: the battery as a `panel_chip` and its percent in
  `mono`, one stop; Screenshot, Settings, Lock and Power as the kit's
  `icon_button`s (ghost, `radiusMd`, `controlHeight`), with GNOME's
  spacers kept.
- A slider row: its mute and arrow buttons the kit's `icon_button`, the
  kit's slider between, `panelRowCompact` tall.
- An open menu: the `panel.group` box it is now; its header's round disc
  becomes a `panel_chip` (lit while the toggle is on) with the title in
  `body` 600.
- Background: a `panel_row` with the `activity` chip, the count as its
  name and a `chevron-right` mark, opening its menu as now.
- The legs read the new numbers off `quicksettings status`: the card 400
  wide, toggles (400 - 16 - 8) / 2 = 188 wide and `panelRow` 44 tall, a
  split's toggle and arrow `rowGap` apart with the arrow 44 square,
  `panelGap` between columns and rows, the body `panelRowInset` from the
  card's sides. The panel-language check (`panel_language_assert`) reads
  the open card like every other headerless card. The dim claim stays
  (the scrim over every box but the menu group's).
- Photograph dark and `--light`, read the frames beside the network panel
  and the Settings page, and fix what does not read as the same family.
- Verify: `just smoke-affected` green (the core set, since `ui/` and
  `fs-theme` change), `just smoke --quick-settings-devices` and
  `--quick-settings-power` on the VM rig, `just lint`. Commit
  `feat(quick-settings): the shell's own chrome on the panel grid`.

Ship to g815 after Task 2 (bump the formalshell rev in
`~/.config/formalspec/modules/linux/store.star`, `fspec update`, commit,
push, `fspec apply --host g815`), restarting `formalshell.service` if the
apply restarted it before the store switched.
