# Pond polish, 2026-11-04 port

**Date:** 2026-11-04
**Status:** planned, on `main`
**Specs:** `docs/superpowers/specs/2026-09-18-theme-boundary.md` and
`docs/superpowers/specs/2026-10-30-panel-language.md` win on conflict.

Pond moved from `pond-2026-10-31` to `pond-2026-11-04` (36 commits, read off
`artifacts/pond/src`, BSD-3; captures in `artifacts/pond/web`). This plan
takes its material polish (casts, the fading outline, the toggle track, the
tile hover, the picker's current mark) and leaves its palettes behind. Pond's
numbers land in `crates/fs-theme/formalshell.json` with a `notes` entry naming
the Pond file they came from; Rust files that port a Pond recipe carry the
`// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)`
line `ui/draw.rs:1681` already uses.

Each task is one subagent, run in order, ending in its own commit pushed to
`origin/main` with the verification output read. Budget legs run on g815 only
(`dev/perf-gate.py` pins `FS_RIG=g815`); on the mac rig they wait for morning.

Out of scope: the sidebar chevron's chip grid. Another agent is rebuilding it
as a GNOME quick-settings card. It sits in the panel host
(`trayoverflow`), so it inherits Task 1's cast, Task 2's outline, Task 3's
switch and Task 4's group hover with no work here.

## Decisions

| Pond change (commit) | Call |
| --- | --- |
| Shadows on battery, calendar, notification centre, toasts (8f16178) | adapt: Task 1 |
| Niri `saturation 1.0`, xray off (0fd9b46, 4612c31) | adopt saturation as Hyprland `vibrancy = 0`: Task 1; xray already off |
| Border opacity 0.4, outline fading to the bottom (6059160, 600d24f `PanelBorder.qml`) | adapt: Task 2 |
| Toggle colour (93b8123) | adapt: Task 3 |
| Menu borders, eased hover (d9f1d2c, cac328b) | adapt: Task 4 |
| Wallpaper picker tiles (cef0f8e, 31f9b43, 781b657, fab1266) | adapt the current mark: Task 5; the rest already ours or skipped |
| Everything else | skipped, reasons at the end |

## Task 1: the raised cast on floating cards

Pond gives every floating card one five-layer cast (`shell/PanelStyle.qml:52-58`
`frontShadows`, drawn by `shell/Shadow.qml`), now on the battery card
(`battery/Window.qml:35-39`), the calendar (`calendar/Window.qml:42-46`), the
notification centre (`notifications/CenterWindow.qml:87-94`) and toasts
(`notifications/ToastWindow.qml:82-87`). Ours carry elementary's shadow(2)
(y3 blur4), which reads flat next to Pond over a busy wallpaper.

- Table: a new `card` state `raised` (`formalshell.json:136`, beside `rest`,
  `opaque`, `dialog`) whose `layers` keep the four HIGHLIGHT lips and replace
  the two SHADOW_2 casts with Pond's five, CSS order:
  `{y:-21, blur:33.5, a:0.06}`, `{y:23, blur:27.5, a:0.08}`,
  `{y:-6, blur:24.5, a:0.12}`, `{y:-2, blur:23, a:0.16}`,
  `{y:6, blur:10, a:0.19}`, all black, alpha `{dark: <Pond's>, light: half}`.
  Pond is dark only; halving for light is our call, written in the note.
  `notification.rest` (`formalshell.json:215`) takes the same five in place
  of its three casts. `card.rest` keeps shadow(2): sidebar cards
  (`ui/draw.rs:919-920`) sit in a column and must not cast this far.
- `surfaces/card.rs:58-66`: `Look::new` reads `box_style("card",
  Some("raised"))` for its casts on both paths (popover, and joined when
  `cast`). `SLACK` (`card.rs:36`, 48) becomes the larger of 48 and
  `cast_reach` (`card.rs:507`) so the deeper cast is never clipped by the
  surface. Toasts already size off `cast_pad` (`surfaces/toasts.rs:285`,
  `:545`, `:799`); confirm their input region stays the cards alone (Pond's
  `mask: Region { item: toastCard }`, `ToastWindow.qml:39`).
- Compositor (`docs/examples/hyprland/formalshell.lua`): `ignore_alpha` from
  0.2 to 0.6 on `formalshell:panel` (`:243`) and `formalshell:notifications`
  (`:325`). Five stacked casts pass 0.2 just outside the card, and at 0.2
  Hyprland would frost a halo round every panel; 0.6 keeps the 0.85 card
  blurred and the cast under the mark, the modal namespaces' reasoning.
  Add `vibrancy = 0` to the blur block (`:60-64`; Hyprland's default is
  0.1696, confirmed on the wiki): Pond's `saturation 1.0`, so the blur
  behind a card stops tinting it toward the wallpaper. Check
  `~/.config/formalspec` for a copy of this block and change it in the same
  spirit through the spec, never on the host.
- Docs: DESIGN.md §1 "Translucency and blur" (`docs/DESIGN.md:77-99`) names
  the 0.6 mark on panel and notifications and why, and the vibrancy. The
  `roles.card` and `roles.notification` notes say where the cast came from.
- Verify: `just lint`; `just smoke --glass-behind`, `--glass-behind
  --no-morph`, `--glass-behind --light` (extend `dev/smoke.d/glass_behind.sh`
  with one claim: a band 8px outside each card over the text frame keeps at
  least 0.8 of the bare frame's edge energy, so no halo is blurred);
  `--panel calendar`, `--panel power`, `--center`, `--notify`,
  `--notify-emerge`, `--panel-emerge`, `--join`, `--card-corner`,
  `--tray-overflow --sidebar` (panel and toast rects still apart); on g815
  `--menu-budget` and `--idle` under `FS_CPU_QUOTA=10%`. If the five-layer
  cast breaks a budget while a card moves, keep Pond's two strongest
  (`y6 blur10`, `y23 blur27.5`) and say so in the note. Read the PNGs.

## Task 2: the fading outline

Pond's `PanelBorder.qml` (new, `:1-30`): a 1px stroke inside the card, full
at the top edge and transparent at the bottom (`strokeGradient`, `:15-20`),
at opacity 0.4 (6059160, `:11`). It replaces Pond's flat border on panels,
toasts, the centre, menus and its close button. We keep the black outer rim
(it separates a card from a dark wallpaper) and turn our lit inner lips into
Pond's fade: today `card` carries top 0.06, bottom 0.04 and sides 0.014 in
dark (`formalshell.json:149-185`), so the change is the bottom and sides
fading out instead of standing.

- `fs-theme/src/style.rs:349` `Ring` gains `fade: Option<f64>` (the alpha at
  the bottom edge as a share of the top), read at `style.rs:463-466` off a
  layer's `fade` key; the schema line in DESIGN.md §1 "The table"
  (`docs/DESIGN.md:100-118`) names it.
- `ui/boxes.rs:84-88`: an inset ring with a fade draws its stroke inside
  `Paint::Faded` (`scene.rs:193`, vertical, over the box's rect). Read how
  `Fade { low, high }` renders before choosing its numbers.
- `surfaces/card.rs`: `Look` (`:42-54`) carries the lit ring too and the
  joined and popover paths draw it inside the outline. Where a reactive rim
  is on (`theme.reactiveRims`, default true), its specular line
  (`surfaces/rim.rs:23`) takes the same vertical fade and the table ring is
  skipped, so a card never draws two inner lines.
- Table: `card.rest`, `card.raised`, `notification.rest` and `menu`
  (`formalshell.json:343`) replace the four HIGHLIGHT lips with one layer
  `{inset: true, spread: 1, color: white, alpha: {light: 0.3, dark: 0.12},
  fade: 0}`. Dark 0.12 sits between Pond's Daylight line (about 0.03 over
  its fill) and its Hazel and North lines (0.16 to 0.24, `themes/Hazel.qml`,
  `themes/North.qml` `panelBorder` times 0.4) and still reads on zinc. Light
  keeps elementary's 0.3 top. Controls keep HIGHLIGHT. The `HIGHLIGHT` note
  says which roles left it and why.
- Verify: `just lint`, `just cargo test -p fs-theme`; `just smoke
  --card-corner` (the top-right arc must not light past the top edge's own
  line), `--reactive-rims`, `--glass-behind`, `--panel network`, `--center`,
  `--notify`, `--gallery`, and the same panel at `--light`. Crop the top and
  bottom 4px of one card and confirm the line is there at the top and gone
  at the bottom.

## Task 3: the toggle track

Pond's track drops its outline for a white wash, `trackAlpha` 0.06 and one
more 0.06 under the pointer (`shell/Toggle.qml:15-16`), 0.12 in a panel
on a translucent theme (`drawerToggleAlpha`, `themes/Daylight.qml:14`). The
slots stay outlines in white 0.3 (`Toggle.qml:27-47`). Its knob swap (bright
knob, dark bar) is what ours already draws.

- Table `switch.track` (`formalshell.json:934-951`): `off` fill `foreground`
  at `{dark: 0.12, light: 0.1}`, `border` null, `ink` `["foreground", 0.3]`
  for the slots, `hover` wash `foreground` 0.06; `on` drops its `primary`
  0.5 border and keeps the off fill: the knob carries the state, as Pond's.
  Our panels are translucent, so the drawer value (0.12) is the base.
- `ui/draw.rs:1110`: the slot colour reads the track's `ink` instead of its
  border; drop the `alpha * 0.6` at `:1119` (0.3 is already the final
  alpha). `:1117` passes a hover fade from `cx.tween` on `Clock::Effects`
  where it passes 0.0 now, the way `cell` does at `:947`.
- Docs: panel-language §6 Toggle (`2026-10-30-panel-language.md:125-131`)
  says the track is a `foreground` wash with no outline in both states.
  DESIGN.md §2's switch row (`docs/DESIGN.md:596`) is stale (46x26, knob 20,
  padding 3): it becomes 52x28, knob 24, padding 2, no outline.
- Verify: `just lint`; `just smoke --gallery`, `--panel network`, `--panel
  bluetooth`, `--toggles`, `--settings-ui`, `--settings-ui --light`,
  `--panel-keys`. Read the switch at rest, hovered and on, dark and light.

## Task 4: the tile's hover outline

Pond outlines its settings tiles and session buttons (d9f1d2c,
`settings/CategoryButton.qml:109-117`, `SessionButton.qml:85-88`) and
eases the fill and outline together on hover, 250ms InOutCubic (cac328b,
`settings/Style.qml` `categoryHoverDuration`). Ours already outline every
`panel.group` and ease the wash on `effects` (200ms), but the outline does
not answer the pointer.

- Table `panel.group` (`formalshell.json:1112-1127`) becomes state keyed:
  `rest` as today, `hover` with the border at `foreground`
  `{dark: 0.16, light: 0.2}` (Pond's `menuCardBorder` white 0.4 times the
  outline's 0.4). Its readers (`ui/draw.rs:914`, `:916`, and any other
  `box_style("panel.group", None)`) keep reading the base.
- `ui/draw.rs:913-916`: an interactive group under the pointer takes
  `hover`; the border already crossfades through `cx.color` at `:945`. Row
  groups are not interactive and do not change. The tiles
  (`surfaces/panel/settings.rs:71-80`) and session buttons (`:98-103`) get
  it with no change of their own.
- Docs: panel-language §4's tiles line (`:107-108`) adds the hover outline.
- Verify: `just lint`; `just smoke --settings-ui`, `--settings-ui --light`,
  `--launcher-mouse`, `--panel info`. Read a hovered tile, dark and light.

## Task 5: the picker's current wallpaper

Pond marks the applied wallpaper with a 26px white disc and a check, 12 in
from the tile's bottom-right corner (`settings/appearance/WallpaperTile.qml:83-92`),
apart from the focus ring. GNOME's background page does the same with an
accent disc. Our picker shows the cursor alone (`surfaces/launcher.rs:3763-3777`),
so the wallpaper on the desktop is not visible in the grid.

- `surfaces/launcher.rs:3763-3777`: the cell whose path is the current
  wallpaper (the picker's status already knows it, `launcher.rs:970`) draws
  a `controlHeight - 8` disc in `primary` with a Lucide `check` in
  `primaryForeground`, inset `sm` from the picture's bottom-right corner. A
  shared `w::` constructor if `ui::w` has no corner badge yet.
- `dev/smoke.d/picker.sh`: after a pick, the new cell carries `primary`
  pixels in its corner and the old one none.
- Verify: `just lint`; `just smoke --picker`, `--picker --light`, `--wheel`
  (the picker's stale audit must stay clean). Read the grid PNG.

## Skipped

- Named themes Daylight, Hazel, North, Wisteria, Unthemed, their switcher
  discs and the launcher theme (a59ed4f, 4ad057c, 0485543, b9aff2a): fixed
  hex palettes, where the theme-boundary spec makes matugen the palette
  source for every theme.
- Panel, modal, calendar and hover gradients (600d24f): the same fixed
  stops, so no gradient fill lands and the banding fix (cf2b409, dither
  shader) has nothing to fix; wallpaper and lock dither already exist.
- Qt rounding at rest (b72da3a): a Qt workaround, vello has no such fault.
- Background panel blur and the new stack animation (5098629, 4e67c38): we
  have no page stack; join and password steps are inline in the card.
- Close button and notification dismiss recolours: our close is the flat
  in-header ghost by the owner's call, the colours Pond's palette.
- Settings menu radius 16 (4e7abd5): our tiles sit inside the launcher card,
  where nested corners are outer minus padding; Pond's float on the scrim.
- Opacity-based solid colours, drawer fills, the checkbox, select field,
  security menu and wifi row tokens (f01e86d, e608646): our hover is already
  a wash of the ink; the amounts are GNOME's and stay.
- Bluetooth pairing screen (3ce41ea): we have no code-entry page, the agent
  confirms and refuses a passkey (`services/devices/bluetooth.rs:402-412`).
- Pond's own settings pages (accordion, switcher, mouse, lock, display,
  information) and the picker's "Choose an image" tile: pages we do not
  have, and no file chooser to open.
- Thumbnail cache, display-resolution thumbnails, no `.avif`: already ours
  (`wayland/launcher.rs:812`, `fs-menu/src/providers/wallpaper.rs:37`).
- Hide unavailable audio devices (2acacbe): behaviour, not polish;
  `fs-audio` reads no port availability yet.
- The included wallpaper set: content, not chrome.
