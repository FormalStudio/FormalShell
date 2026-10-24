# FormalShell design language

The shell is Omarchy quattro's set of surfaces and habits, drawn as shadcn/ui
in its dark and light themes, with the wallpaper filling the palette through
matugen. The full token tables and the reasoning are in
`docs/superpowers/specs/2026-08-25-shadcn-omarchy-redesign.md`; that spec
wins if this file disagrees with it. This file is the working rulebook: what
to reach for when building or restyling a surface.

Paths below are relative to `crates/`. The shell is `formalshell-rs`; its
widgets live in `formalshell-rs/src/ui/`, its surfaces in
`formalshell-rs/src/surfaces/`, and the theme logic they read in `fs-theme`.

## 1. Tokens

Every value below comes off the live `Theme` (`fs-theme/src/theme.rs`), with
the token maths in `fs-theme/src/tokens.rs`. A surface never writes a literal
colour, radius, size or duration.

**Color** (`theme.colors.get("<role>")`, the roles in
`fs-theme/src/palette.rs`): `background`, `foreground`, `card`,
`cardForeground`, `popover`, `popoverForeground`, `primary`,
`primaryForeground`, `secondary`, `secondaryForeground`, `muted`,
`mutedForeground`, `accent`, `accentForeground`, `destructive`,
`destructiveForeground`, `warning`, `warningForeground`, `border`, `input`,
`ring`, `chart1`..`chart5`. matugen fills them from the wallpaper; the
fallback is shadcn's zinc palette, and a wallpaper with a pinned palette's
name in its path (`flexoki`, `zenbones`) pins that palette instead, for the
shell and for every app template alike (`fs-theme/src/flexoki.rs` and
`fs-theme/src/zenbones.rs` each carry all three views of one table). The
wallpaper colour is `primary`.
`accent` fills a selected row and a list's own cursor, never the wallpaper
colour.

**Hover and press** (`Theme::hover_fill`, `Theme::press_fill`): a wash of
the ink, not a fill. Every surface the pointer reaches is drawn at
`surfaceOpacity`, so an opaque chip on top of one lands at a delta the
wallpaper decides: a bright wallpaper lifts the surface past `accent` and
the hover reads as a dark patch, one close to `card` leaves no delta at
all. The wash stacks on whatever resolved there instead and keeps its size
and its direction over every wallpaper. A control already carrying a
colour takes `Theme::hover_filled(c)`/`Theme::press_filled(c)`, the fill
blended toward `background` and left opaque (shadcn's `hover:bg-primary/90`);
dropping its opacity instead makes it see-through on a translucent panel.

**Radius** (the ladder, `fs_theme::tokens::radius_tokens`): `Sm` 6, `Md` 8,
`Lg` 10, `Xl` 14, `Full`. `theme.radius` in `settings.json` moves the base
(default 10); the others follow. Nested corners are outer minus padding,
floored at `Sm`.

**Border**: 1px `border`. `input` on text fields. No other width exists.

**Ring**: focus is `border` swapped to `ring` plus a 3px outer halo of `ring`
at 0.5 alpha (`Theme::ring_width`). Same drawing on every surface, and the
keyboard's own mark: it draws only while the cursor's last move came from a
key, and a cursor the pointer put on a row leaves that row its hover wash
alone, which is the whole mark a pointer needs. The row keeps the cursor
either way, so Enter still acts on it. The surface owning the cursor tracks
whether its last move came from a key (the panel host's `keyed` flag in
`formalshell-rs/src/surfaces/panel/host.rs`, decided by
`fs_chrome::cursor::ring_after`) and every row under it draws by that one
flag. The halo falls outside the item's own bounds, so a clipping container
reserves room for it: it grows its clip rect by `ring_width` on every side
and insets its content by the same, which leaves every row at the x, width
and top it had without a ring. The rule lives in the container (the panel
host's content viewport, the launcher's and the centre's lists), never in
the surface, and a row never insets itself.

**Translucency and blur**: the bar strip, panels, the launcher card and the
polkit consent card paint `Theme::surface(card)`, the card colour at
`theme.surfaceOpacity` (0.85). Hyprland blurs what is behind them
(`hl.layer_rule` `blur` + `ignore_alpha` on `formalshell:bar`,
`formalshell:panel`, `formalshell:menu`, `formalshell:polkit`, and the rest,
in `docs/examples/hyprland/formalshell.lua`; mango blurs them through
`blur_layer` in `formalshell-chrome.conf`); the shell itself never blurs
outside the lyrics pane's own depth of field and sung-chunk glow (owner,
2026-09-17, gated behind `media.lyricsBlur`). A translucent card with
nothing blurred behind it reads as a rendering fault rather than as depth,
so the two travel together: a surface is either on that list or opaque.
Toasts and lock are the opaque ones; the OSD joined the blurred list when it
started budding off the bottom line (M57 D8), since one silhouette across
two windows cannot be opaque on one side of the seam. The modal namespaces
(`formalshell:menu`, `formalshell:polkit`) each cover the whole output and
carry a plain black 0.5 scrim, so they take `ignore_alpha = 0.6` rather than
0.2: the scrim falls under the mark and only darkens the desktop, while the
card over it stays above and keeps its blur (owner, 2026-09-17: "make the
overlay just darken instead of blur").

**Themes**: the chrome every primitive draws comes out of one table,
`fs-theme/themes/<name>.json`, one file per theme, embedded by
`fs-theme/src/tables.rs` (`metamorphosis` is the look above; `retro` has no
file and resolves to it, since retro differs from it only by the scalars
`theme.preset` already resolves). JSON carries no comments, so each table's
reasoning sits in its own `notes` key, which nothing resolves. A table is
keyed by role, and each role is one box: `fill` (a colour role or a
literal), `fillAlpha` (a number or `"surface"`), `radius` (a step off the
ladder or a literal), `border` (`{ color, alpha, width }` or none), `face`
(the one top-to-bottom gradient a material has, or none), `layers` (CSS
`box-shadow` order, a hairline, a ring or a cast depending on its own blur
and spread), `tint` (a colour blended into an opaque fill under the pointer,
never its alpha dropped), `wash` (a translucent surface's own hover and
press, a colour laid over the fill rather than blended into it) and `edge`
(a hairline drawn outside the box, the bar strip's line against the
desktop). Any alpha may instead be `{ light, dark }`, resolved by the live
palette's mode. A role's states merge over its base state key by key.
`habits` in the same table are the same idea for shape rather than chrome:
which surface a bar, an emerging card or a notification takes where a theme
differs from metamorphosis in more than colour. A primitive renders its role
and state and never names a theme; a theme never reaches into a primitive.
`Theme::box_style(role, state)` (resolved by `fs-theme/src/style.rs`) is the
only way chrome reaches one, drawn by `ui::boxes::paint`
(`formalshell-rs/src/ui/boxes.rs`).

**Pantheon** (`fs-theme/themes/pantheon.json`): elementary OS 8's material
as the same kind of table, transcribed from its own GPL stylesheet rather
than ported, each block naming the file its numbers came from
(`_exported.scss` for the tokens, `_index.scss` for the mixins,
`widgets/*.scss` for the roles). GTK's `alpha(c, f)` multiplies rather than
sets, and the `highlight` base is white in light but white at 0.2 in dark,
so every highlight-derived alpha in the dark column is written as that
product: `alpha(@highlight_color, 0.3)` lands at white 0.06, not 0.3, which
is why the dark material reads quieter than the light one rather than as
the same lines at a lower opacity. Radii are elementary's own, pinned by
number rather than by the radius ladder's step so `theme.radius` moving the
base never drifts them: 3 for a control, 6 for a popover, 9 for a card or
bubble. Its surfaces are opaque (`theme.surfaceOpacity` 1): elementary's
popovers and dialogs are `bg_color(2)` with nothing behind them, and its one
translucent surface is the panel, whose alpha belongs to the band's paint
rather than to that key.

**Cartridge** (`fs-theme/themes/cartridge.json`, spec
`docs/superpowers/specs/2026-10-23-cartridge-theme.md`): an old console's
menus after tsubi.club, flat and dark, and the one table allowed past the
theme boundary spec for its panels, backdrop and session start. Boxes are
square with one corner cut (`chamfer`: one number for every corner, or four
clockwise from the top left, drawn as that polygon in place of the radius),
outlined at 1px, sunk rather than cast, with the palette's `primary` as the
one accent. Its pills (buttons, the status strip, tags) are why its base
radius stays at 4 while every box pins 0. The cursor has no ring: a list
row takes `cursor.bar`, a block on its leading edge `width` across that
travels with the list's halo, over the `cursor` role's faint fill; a grid
cell (launcher grids, the switcher) takes `cursor.brackets`, the cell's own
outline held `gap` outside it and drawn only where it lies within `sweep`
degrees of straight up or down from the centre (measured as if the cell
were square, so a wide cell gets corner brackets), in the `border` line
over a soft glow of that role's cast (`Theme::with_brackets`). `hero` frames a panel's hero, with
`hero.tab` (`height`, `slant`) its label over the frame and
`button.default.cap` (`width`) the hexagonal end a primary action carries.
Every other table declares all five and draws none of them (a bar under a
pixel, brackets with no line, a tab and a cap 0 wide, a hero that is the
ghost cell it always was). Its fonts block names Archivo for words and
headings, DotGothic16 for values, BIZ UD Mincho for secondary lines, all
shipped with the shell. Its habits are `emerge: "slide"` (the popover card
sliding 40px in along its line from the end it rests nearer, fading on the
table's 900ms `emerge` clock), `panel: "console"`, `grid: "hex"` and
`boot: "cartridge"`; the others declare `sheet`, `square` and `none`.

Three more roles are cartridge's alone and transparent everywhere else.
`bar.cell` is the dark chip every bar cell sits on (the strip itself,
`bar.rest`, draws nothing), with the hovered or open cell taking the
cursor's fill and leading block; a changed label (the clock on the minute)
types in a character at a time. `launcher.letter` is the cursor row's
initial, giant and faint past the launcher's far corner, crossfading on its
`duration`; `grid: "hex"` turns the launcher's grids into pointy-topped
hexagons. `lock.clock` sets the lock's clock in the display face at its
`wdth` and `wght`, slamming in from `slam` of the screen's width. The row
toast becomes an "updates" card on the console grammar (the id strip, the
summary in caps, the body in the values face, a red pulse on a critical
one), the OSD's icon takes the accent, and the switcher's thumbnails take
`switcher.cell`'s chamfer.

`panel: "console"` is the section page (`surfaces/panel/host.rs`): the
panel's `title` wide and heavy in the display face (`panel.title`'s `wdth`
and `wght`), its `subtitle` condensed after `//` with a glow
(`panel.subtitle`'s `inkShadow`), the back chip (Escape's keycap and
"Back") in place of the close button, the status strip under them
(`panel.status`: `<ID> :: <status>` and `( yyyy / mm / dd )`), and the id
strip down the card's leading side (`panel.strip`, `width` wide, its line
also the card's outline), which widens the card rather than narrowing the
body. A `ui::w::heading` (every `w::section`'s label and the field labels
naming one value) draws as a field tag (`panel.tag`: its caps over an
accent bar fading out over `trail`); a plain `section_label` (a state word,
an empty state, an error) stays as it was. Each panel names its accent
(`Panel::accent`: `primary`, or matugen's secondary and tertiary through
`chart2` and `chart3`), and the console draws the whole card with
`primary` taken from it; an accent under 3:1 against `background` (the
static fallback's grey chart steps) keeps `primary`. Every other table
declares the five `panel.*` roles transparent and keeps the sheet.

`boot: "cartridge"` is the session start (`boot.rs` for the timeline,
`surfaces/boot.rs` for the drawing). On black, the desktop's own grid lines
grow in from either end, two rings spin in squashed and settle into the
emblem, and the hostname over `FORMAL` drops in a letter at a time in the
display face, all in the `boot` role (its fill the ink, its border the
lines; white on every table, since the stage is always black). The greeter
plays it under its card, the emblem perching above the card once it has
assembled; logging in crumples the emblem and the session only lifts the
black (`FORMALSHELL_BOOT=greeter` in the session's environment). A session
with no greeter plays it on an Overlay layer per output, the first taking
the keyboard: any key or press goes to the way out, and left alone it goes
600ms after the emblem has assembled. The bar waits under it and slams in
from `0.06` of its width on the lift, and the backdrop's sculpture starts
turning once the overlay is gone. It plays once per login: state.json's
`boot` holds the kernel's boot id and the compositor's instance, so a shell
restarted in the same session shows the desktop at once. `theme.boot`
turns it off. The unlock has the short form whatever that key says: the
clock slams back out and the card fades while the session is still locked,
then an overlay holding the lock's backdrop (`formalshell:unlock`, no input)
lifts off the desktop over 900ms. Every other table declares `boot: "none"`
and the `boot` role it never draws.

`sounds` is the table's map from an event to one of nine blips: `move`,
`activate`, `back`, `open`, `close`, `type`, `toggle`, `error`, `boot`.
They are original, synthesised by `dev/sfx/gen.py` into
`crates/fs-audio/sfx/` (48 kHz mono WAVs, embedded) and mixed in process
on a PipeWire playback stream of fs-audio's own (`formalshell-sfx`, role
`Event`), opened for a blip and closed 1s after the last one ends, so a
quiet shell holds no stream and wakes for none. The client thread keeps that node and its
links out of the graph it reports, so no panel lists it. The UI thread
only sends a command (`App::sfx`): one blip per frame at most, `type` at
most every 50ms, none under do-not-disturb, none while locked except the
lock's typing, wrong password and unlock. The launcher's blip is read off
what its key changed (`sfx::launcher_cue`), so an activate that also
closes plays the activate alone. `theme.sounds` turns a table's own off
and `theme.soundVolume` sets the level on the visual volume scale. A table
without `sounds` is silent; only `cartridge` has one.

**Type** (`fs_theme::tokens::font_tokens`, picked per element with
`ui::Type` and `ui::Weight`): sans for words (titles, labels, buttons,
section labels, descriptions, hints), mono for values (numbers, units,
times, identifiers, chords, clipboard and terminal content). Both are
fontconfig aliases, Geist Sans and Geist Mono by intent. Sizes `caption` 11,
`bodySmall` 12, `body` 13, `subtitle` 14, `title` 15, `heading` 17,
`display` 26, `displayLarge` 30. Weights `normal` 400, `medium` 500,
`semibold` 600. Values that change are mono, so they stay tabular.

**Space** (`fs_theme::tokens::Space`): the raw steps are `xxs` 2, `xs` 3,
`sm` 4, `md` 6, `lg` 8, `xl` 10, `xxl` 12, `huge` 18; the semantic keys are
`controlHeight` 32, `barCellHeight` 28, `barCellWidth` 44, `barMargin` 6,
`controlPaddingX` 12, `controlPaddingY` 6, `rowGap` 4, `iconGap` 8,
`panelPadding` 12, `sectionGap` 16, `trackThickness` 6; `screenPadding` 12,
`popupWidthNarrow` 320, `Default` 380, `Wide` 480, `Menu` 560, `MenuSplit`
840, `MenuApp` 900. `panelPadding` is the gap inside a card, `screenPadding`
the gap outside a floating surface; they share a value and describe
different things.

**Separation**: one ladder, five rungs, and a boundary takes exactly one of
them. Reaching for a rung is a claim that the one below it was tried and was
not enough, so the question is never "does this need a box" but "how far up
does this boundary have to go".

1. **Nothing.** Rows inside one group. A row is `controlHeight` tall with
   `controlPaddingX` either side, and that padding is the gap. Rows in a
   uniformly interactive list abut with no gap; the hover wash and the
   cursor ring are what a pointer needs, and neither is a resting mark.
2. **Space.** `sectionGap` between sections against `rowGap` inside one, a
   4x ratio, so the grouping reads before anything is drawn at all. This is
   the rung most boundaries stop at, and a boundary that still reads wrong
   with `sectionGap` under it usually wants a name, not a line.
3. **A name.** A section label (`w::section_label`) over the group, inset to
   the row text under it. A heading both separates and says what the group
   is, which no amount of space or ink can do, so a group worth separating
   is usually a group worth naming.
4. **A rule.** A separator (`w::separator`), a 1px `border` line, full-bleed
   across the surface or inset to the row text (`w::separator_inset`). For a
   seam space cannot carry: two halves of one surface that differ in kind
   (the monitor ledger above its process table), a header or footer against
   the content it frames, a divider a data source itself declares (a D-Bus
   menu's own separator entries), or a list whose rows are taller than one
   line. Never directly under a section label, which is rung 3 doing the
   same work twice.

   That last case is where rung 1 runs out. Rung 1 holds while every row is
   one line tall and the gap between rows is the largest gap in sight. A row
   carrying its own stack (the notification centre's sender line, summary,
   body and actions) has internal gaps as big as the gap to the next row, so
   space stops reading as separation and the list runs together into one
   block of text. Those rows take a rule between them. The test is the row's
   height, not the list's length: uniform `controlHeight` rows still abut
   with no gap and still take nothing.
5. **A card.** The `card` role. Every floating surface is one, drawn at its
   outer edge: the panel's frame, the launcher's, a toast's, the OSD pill,
   the notification centre's. Inside that frame a surface may spend one
   more, on the single block the rest of it points at, and it never spends
   that twice. Nothing goes a level deeper: a card inside that card does not
   exist, and a block that needs marking off inside one takes rung 4 or
   lower however much it outranks its neighbours (owner, 2026-08-26: "no
   nested cards anywhere, just a panel, and max one card", "one card for the
   preview so it didn't count as nested").

   A surface here is a thing that floats with nothing behind it, not a
   window. The capture overlay is transparent and full-screen, so the blocks
   inside it (the region readout, the toolbar, the unnamed-window list) each
   sit on the raw desktop and each is its own frame. Two cards on that
   overlay are two surfaces, not a surface spending twice.

   Where each surface spends the allowance, or doesn't. The launcher's split
   route spends it on the preview pane: flat rows down one side and the card
   down the other, so the pane reads as the thing the list is pointing at. A
   panel spends it on nothing, save the media panel, which spends its one
   card on the lyrics pane beside the now-playing column (M56 P13), the
   launcher's own preview-pane grammar reused rather than a second shape:
   otherwise the header's rule and `sectionGap` already rank the hero over
   the sections under it, and the border the hero used to draw was the "too
   big usage of cards everywhere" the owner called out. The notification
   centre spends it on nothing either: its rows are the list, not a block
   the list points at.

   What a resting box actually marks decides whether it is one. A fill and a
   border at rest say *the pointer or the keyboard acts on this*: a button,
   an input, a switch, a button group's trough, a chip cell, a lone
   clickable row that is not part of a uniform list. That is chrome on a
   control, and it never counts against the one card above. A block that
   only groups its children, or only reports a value, earns no box at any
   depth.

   An outline around imagery is not a card either. Album art and a
   notification's app icon keep a 1px frame, because a picture bleeding into
   the surface behind it has no edge of its own. A cover (`w::cover`) draws
   exactly that frame and nothing else, and rounds the picture to the same
   corner: `Theme::cover_radius(extent)`, a quarter of the slot's shorter
   side capped at `radiusSm`, so the bar's 17px art is a rounded square
   rather than a lozenge (owner, 2026-08-26) and a 96px panel cover lands on
   the ladder. Everything that wants no edge stays a plain picture
   (`w::picture`).

**Padding**: one rule, on every surface. A card insets its content by
`panelPadding`. A row is `controlHeight` tall with `controlPaddingX` either
side and its content vertically centred; it is taller only when its own
content needs the room, and a badge that sits inside a row rather than
being one (a chip cell, `CellState::chip`) hugs its label. A section label
sits `sectionGap` below the block above it and `rowGap` above its rows, and
the rows inside a section are `rowGap` apart. An icon and the label beside
it are `iconGap` apart. A header row is `controlHeight` tall and takes the
same horizontal padding as the rows under it: none of its own where those
rows draw their own border, `controlPaddingX` where they do not, which is
why the launcher's input row and footer line up with its row labels rather
than with the card edge. A section label follows the same rule against the
rows it heads, so a section of flat rows takes `controlPaddingX` and a
section of bordered rows takes none. A floating surface sits
`screenPadding` off the screen edge it hangs from and `barMargin` off the
bar or the item it is anchored to, centred on that item along the bar's own
axis and clamped to the padding at either end, and is no taller than the
screen minus the bar and those two paddings: past that its content scrolls
under the wheel rather than the surface running off the display. Toasts,
the OSD pill, the notification centre and the tooltip take those same
numbers. A surface never writes its own margin.

**Motion** (`fs_theme::tokens::motion_tokens`, run by
`formalshell-rs/src/motion.rs`): two families of clock, and the property
picks the family, never the surface. `spatialFast` 350, `spatial` 500 and
`spatialSlow` 650 pace anything with a position or a size: x, y, width,
height, margins, scale, radius, rotation, an emerge, a morph, a cursor's
travel. `effectsFast` 150, `effects` 200 and `effectsSlow` 300 pace anything
with neither: opacity, colour, a progress that only drives alpha. Two names
sit outside both families, `emphasized` 400 for the workspace pill and
`emphasizedDecel`, a curve with no duration of its own that pantheon's
bubbles restack on. The row toast keeps sonner's own clock instead, 400 on
CSS `ease` (the table's `arrive` and `restack`), which never passes rest.
`reveal` 400 is the
full-screen fades: the wallpaper and palette crossfades, the lock's blank
and wake, the screensaver's own enter and exit, on `effectsSlow`'s curve.

The curves are Material 3 Expressive's, the set caelestia runs on (amended
2026-09-09, owner: caelestia's morphs, not curves alone). Each is a cubic
bezier (`motion::Curve`, the points in `fs_theme::tokens::MOTION_CURVES`):
every spatial curve carries a y control point above 1, so what travels
passes its rest by a few pixels and settles back on it. The effects curves
never overshoot. A spatial kind put on an opacity is clamped instead, which
reads as the fade stalling just short of the end. `motion.enabled=false`
zeroes every duration and pins the deform to identity, so every surface
lands at once, at rest and undeformed; the curves themselves are left alone,
a zero-duration animation ending in the same place whatever it carries.

A surface names a clock exactly one way. A number rides
`Cx::tween(key, target, kind)` (`formalshell-rs/src/ui/mod.rs`), `kind` one
of `motion::Kind`'s six family members, `Kind::Spatial` unless the property
says otherwise; outside the widget tree, a number is a `motion::Animated`
on the same kinds. A colour rides `Cx::color(key, target)`, always
`effectsSlow`, since a colour has no position to overshoot and a crossfade
shorter than that reads as a flicker rather than as a change. A list's rows
take the same clocks: a row that keeps its place moves on `spatial`, an
arriving one fades in on `effects` and a leaving one fades out on
`effectsFast`.

Past its clocks a table may name effects (`fs_theme::style::Catalogue`,
the maths in `formalshell-rs/src/effects.rs`), each absent on a table that
does not carry it, so metamorphosis and pantheon run none of them:
`focusText` (the cursor or hovered row's label going to a width, weight
and tracking on its own clock), `reveal: "type"` (a hero's title and
readout, a console's status line and any `El::typed` text typing in on
`steps(20)` over 600ms, placed by its whole width), `arrive.rise` with
`arrive.stagger` (a card's rows rising in on open, on the `emerge` clock),
`swap: "glitch"` (a launcher level change's rows jumping 10px back and out
in two steps, the new ones coming in from 20px ahead on the `arrive`
curve; a panel handoff draws the way in alone), `use` (a ghost of the
activated row's label scaling and fading), `error: "shake"` (a field's
error label jittering its axes over 500ms), `picture: "zoom"` (a picture
that changes landing from 20 times its size in four stairs), `noise` (a
grain over a fresh card fading out) and `progress: "blocks"` (a readout as
`█████░░░░░ 50%` in the values face). Every one of them runs off a moment
(the open, the change, the activation) and lands; none runs at rest.

An edge-anchored card and the line it comes out of are one silhouette while
it comes out (amended 2026-09-09, and 2026-09-14 for the let-go). The bar
draws a single 1px `border` along its inner edge, the frame ring one round
its cut-out, and a card coming out of that line opens a gap in it: the
card's own rect plus the fillets' reach at either end (`radiusXl` while
attached), a plain function of the card's live rect with no clock of its
own, since the card's own clocks already carry that rect and a second one
left the line lagging the shoulders it has to meet. The shoulders
(`formalshell-rs/src/surfaces/shoulders.rs`) draw the card with that edge
left open and a concave quarter fillet outside each of its two corners,
running out to where the line resumes, so the card grows out of the bar
rather than parking against it. Attached, the fill starts a border's width
in from the line rather than on it (M57 D1): that row belongs to the line's
own window, which paints it already, and a second translucent fill over it
read as a seam across the whole gap. The fill's fillet then shares its
centre with the stroke's, concentric at the join's own radius against that
radius plus half a border, rather than the two arcs sitting a half pixel
apart. The card publishes its join (the line's edge, the card's x and width
along it, the reach) off its frame's live position (`Card::joins` in
`formalshell-rs/src/surfaces/card.rs`, gathered for the line by
`App::sync_join` in `formalshell-rs/src/wayland.rs`), so a size morph and a
handoff carry the gap with them frame by frame; that rect is the only thing
the two windows share, and it is gone once the card has let go (§1 Motion).
`theme.radius` 0 draws square corners and no fillets, which is a plain card
against a whole line. The panels take the shoulders, and the chevron's and
the tray's second bars with them, on a framed screen as on a bare one: the
ring's hairline runs the bar's edge there and opens the same gap; the
notification centre takes it against the ring's far side or a right bar.
The OSD pill takes it too against the bottom line (M57 D8), a
`screenPadding` off whatever that edge carries; on a bare bottom edge there
is no line to join and the pill comes out from behind the output.
Everything that meets no line keeps a plain card: the toasts and the
tooltip float with nothing for a fillet to run out to.

A card that arrives squashes into the edge it came from (`motion::Deform`,
amended 2026-09-09). Its scene position and size are sampled every frame,
the velocity taken from the delta, and a matrix stretches the card along the
direction of travel and compresses it across, so the area holds. Each
component of that matrix rides an underdamped spring, so the card keeps
deforming for a beat after the travel has stopped and unwinds through its
rest instead of snapping to it. The stretch is capped at 35% however fast
the card goes (`fs_theme::tokens::DEFORM`), and an amount sets how much of
it a surface takes: 0.25 on the OSD, 0.15 on a popout, 0.1 on the launcher.
The matrix is centred on the anchored edge's midpoint rather than on the
card's own centre, so a drawer squashes into the bar and keeps its top edge
on the line it hangs from. At rest it is identity and no frame is
requested, so a still shell costs nothing.

An anchored surface opens as a drawer (amended 2026-09-09, owner: the fade,
0.97 zoom and 8px slide it replaces read as the surface not animating at
all). A panel, the notification centre and the OSD each hang off one edge:
the card starts behind that edge, displaced toward it by its own extent on
that axis, and travels to rest on `spatial` both ways, clipped at the line
it rests on so it comes out from under the bar rather than across it. No
fade and no zoom, since the clip is what hides it; its contents come up on
`effects` behind the travel, so the card lands before its text. A card's
silhouette does not travel with it (amended 2026-09-14, owner: caelestia's
popouts and Material's container morph; a drawer whose shoulders rode
behind the line slid out of a slot in the bar with an empty gap between the
two for the length of the travel): the shoulders are drawn from the line to
the card's far edge, the fillets on the line from the first frame with
their radius capped at that depth, so what comes out is one shape budding
off the strip while the contents slide out inside it, and the deform pivots
on the line rather than on the card's own edge, so the card squashes into
the bar. Then it lets go (the metamorphosis, owner: the card comes morphing
out of the bar and is then its own thing): as the pose passes 0.85, into
the overshoot, `attach` runs to 0 on `spatialFast`, the fillets shrinking to
sharp corners and rounding out the other way, the near edge pulling off the
line to its resting margin with its border coming up, and the line's gap
closing in from both ends under the card. At rest a panel is a plain card
one `barMargin` off a whole line; a close runs the whole thing backwards,
the card reattaching as it slides back under.

A side of the card resting closer than the join's own radius to where its
line ends has no room for a fillet (M57 D2, walls): attached, the silhouette
simply runs out to that wall instead, taking the corner the wall's own line
ends in there (a frame ring's own radius; square against the output's bare
edge) and pulling back off the wall on the same attach clock the near edge
pulls off the line by. A walled side publishes a second join on the wall's
own edge, so a frame ring opens its line there too. Which sides are walled
is decided off the card's resting rect, never its live one, so a card
mid-emerge or mid-handoff never walls and unwalls itself as it travels.

A panel takes its line from the bar, framed or bare. A panel hanging off
another panel (a tray item's menu off the tray's second bar) takes that
panel's far edge instead, which opens the same gap in its own border, and
buds from what that edge can give it (M57 D3, the nested bud): the
silhouette's rect along the line is clamped into the edge less its own
corners at either end, widening to the card's own rect on the attach clock,
so a card wider than the strip it hangs off never hangs its fillets past
either end of it. A span too tight for two fillets and a sliver of card
leaves it joining nothing, coming out plain from under the owner's edge
instead. The notification centre takes the frame ring's far side, or the
bar's own hairline when the bar is on the right, and comes out from behind
the output's edge as before when there is neither. A card that floats in
the middle of the output does the same thing off the top line (M57 D5): the
launcher, the polkit request and the radio atlas all take the top bar's
hairline, the frame ring's top line, or the output's own top edge with
neither, and the depth back to it is hundreds of pixels rather than a
panel's handful (`formalshell-rs/src/surfaces/modal.rs`). No fade, no zoom.
The let-go mark and the deform's own squash both scale back by how deep
that line lies against the card's own size across it, so a card hundreds of
pixels down the output releases earlier and squashes no harder than a panel
a handful off the bar: nothing about a deep drawer runs any slower for how
far it has to travel. Its scrim reads the same drawer's own pose rather
than a clock of its own (`card::Scrim`): plain black at 0.5, except over
the band the card's own line belongs to, whose share rides `1 - attach`, so
the card buds off a lit bar and the bar dims only as the card lets go of
it. The card's contents are laid out at its settled size from the first
frame and the card's own cut is what reveals them, so a level under the
launcher's rule is uncovered by the far edge rather than pushed into place;
that level takes no fade of its own, and a level change while the launcher
is already open plays one `effects` fade in with no out half, since the
route is resolved by the keystroke that asked for it and the body has
already changed by the time an out half could run. The tooltip and the
bar's own reveal keep the fade recipe, opacity on `effects` and scale from
0.97 on `spatialFast` from centre. Every one of them waits for its window
to be on screen before it starts (`Card::mapped`): a compositor can spend
most of an enter putting the surface up, and an animation that ran behind
it would land already at rest.

One type owns this whole recipe (`Card` in
`formalshell-rs/src/surfaces/card.rs`, M57 D4, over the pure geometry in
`fs-chrome/src/drawer.rs`): a consumer states only the edge it comes out of
and its resting rect, and the card derives where that edge's line lies, the
depth back to it, which sides are walled, the span an owner's edge can give
and the deform's two pivots, and runs the pose, the let-go, the deform and
the shoulders underneath. Panel, the notification centre, the launcher, the
polkit dialog, the radio atlas and the OSD all sit on it, so a surface that
moves, resizes or changes edge at runtime is followed without being told.

Opening a panel while another is open is a handoff, and it runs on one clock
(amended 2026-09-09). The new card is drawn on the old card's rect, the old
window is cut on the tick the travel starts, and the one card left travels
and resizes to its own place on `spatial` while its contents crossfade on
`effects` over the first part of that same movement. One eased parameter
carries the rect rather than four animations over x, y, width and height:
the destination stays live, so content that settles its height a frame late
moves where the card is going instead of stranding the travel short of it,
and the four components ride one curve either way. The shoulders travel
with the frame and the bar's gap follows them.

Geometry never jumps (M53). Anything whose x, y, width or height changes
while it is on screen animates the change: a row that moves travels, a list
adds and removes rows through their own fades, and a container whose size
follows animated children rides their clock. Sizes follow content live on
`spatial`, a panel whose section count changes and the launcher's list under
a query alike, and a closing surface freezes its size first. That rule has
one shape wherever a card's own width or height is the thing changing (the
panel host's `morph` in `formalshell-rs/src/surfaces/panel/host.rs`, and the
launcher's own in `formalshell-rs/src/surfaces/launcher.rs`, M57 D7): the
target tracked while open, frozen on close, and armed off the surface being
mapped rather than its enter having settled, so content landing a tick after
the open (a Wi-Fi scan, a keyed row sync) retargets the running clock
instead of jumping through a gate a settled surface would already have
closed. Bar cells enter and leave through their own presence while the
strip's rails carry the neighbours. The launcher's rows keep their identity
across a query, so a re-rank moves rows instead of rebuilding them; only a
level change or a diff touching more than 64 rows (`ROW_RESET_LIMIT`)
resets the list.

Content never snaps. What one slot draws crossfades on `effects` when it
changes: an icon glyph, a label under a width morph, the launcher's empty
state, the clipboard preview, a calendar month. A fill or border that
switches which colour role it binds crossfades through `Cx::color`. A
palette change (mode toggle, matugen recolour, preset swap) crossfades every
colour role over `reveal`.

The cursor travels (amended 2026-09-09, owner: a keyboard user wants what a
keystroke moves to be seen moving). It is one item per list, the launcher's
`accent` row and a panel's ring, moving on `spatialFast`; a one-step move
travels, while a wrap, a filter reset and a level change snap, since nothing
connects the two positions. A selection fill (the segmented control, the
picker's mode tabs) travels the same way on `spatial`. Everything a key
drives is a retargeting tween, never a restarting animation, so key repeat
glides. A toast is the carve-out: it moves the way sonner's does, rising out
of the anchored edge by its own height as it fades in and dropping back
toward it as it fades out (the whole height for the front card, 40% of it
for one behind), with the pile restacking on the same 400ms `ease`, no
overshoot, no stagger and no deform. The workspace pill keeps its own: both its
edges take `emphasized` and the trailing one runs at twice that clock, so
the leading edge reaches the new slot while the trailing edge is still
leaving the old one, which is what makes the pill stretch across the gap
and close up behind itself
(`formalshell-rs/src/surfaces/bar/cells/workspaces.rs`).

Tooltips are one card per output (`formalshell-rs/src/surfaces/tooltip.rs`).
It appears 400ms after the pointer parks on a cell and, within 500ms of
leaving, follows the pointer to the next cell instead of paying the delay
again: the card travels and morphs its width on `spatial`, its text
crossfading on `effects`.

**Startup** (M52): nothing paints before the shell knows what it looks
like. The boot surfaces (bar, background, frame) hold their windows
unmapped until the config, the palette and the plugin scan are all in, each
of which counts as in on its failure branch too, with a 400ms backstop so a
wedged read can never keep the screen bare; then each maps once at final
geometry, one exclusive-zone publish, one tiling shift. The bar's content
enters from its own edge, and only after that reveal settles do the cells'
size tweens arm, so a service answering late is laid out rather than
animated in. The first real wallpaper hard-cuts (`reveal` is for changes,
not boot), and a theme republish that changes nothing writes no file and
reloads nothing.

**Imagery**: content pictures go through `w::picture`, a bare bitmap plus
the retro pass `theme.dither` turns on (`fs-chrome/src/dither.rs`). A
picture that needs an edge of its own is a cover (`w::cover`). Neither is
chrome: an app icon and an album cover keep their own colours on a filled
row, unlike every other ink on it.

**Icons**: `w::icon("wifi")`, resolved through the set `theme.icons` selects
(`lucide` default, `nerd`) in `fs-theme/src/icons.rs`. Size equals the
neighbouring text's font size. No raw codepoints in surface modules.

**Presets** (`theme.preset`, `fs-theme/src/presets.rs`): a table of
defaults for the chrome knobs, never a mode a surface can read.
`metamorphosis` (the default) is everything above. `retro` is the shell's
earlier language as a setting of this one: `theme.radius` 0, `theme.icons`
`nerd`, `theme.fonts` `mono` (words take the mono face too),
`theme.surfaceOpacity` 1, `theme.blur` false, `theme.dither` true. An
explicit key wins over the preset. `radius` 0 means 0 on every step and
every pill (`Theme::pill_radius`), whichever preset set it. `theme.dither`
on renders content imagery (launcher and active-window icons, notification
images, album art) through the retro pass, paints a track's groove as a
dither fill, and is the default for `wallpaper.dither` and `lock.dither`;
tray icons, the picker grid and clipboard thumbnails stay true colour. `theme.grid`
(on under `cartridge`) is the other full-screen pass: the table's
`backdrop.grid` role (pitch, width and fade in logical pixels, its fill the
ink) laid into the wallpaper's pixels by `fs-chrome/src/grid.rs`, and drawn
over the lock's scrim and the greeter's wallpaper from the same geometry;
`wallpaper.grid` and `lock.grid` override it the way the dither keys do.
`wallpaper.color` (`#402484` under `cartridge`) makes the desktop a flat
field of that colour: `services/wallpaper.rs`'s `source` is the one answer
to what the desktop shows (the picture's path or `color:#rrggbb`, the
picker's choice in `state.json` first), and the backdrops, the lock, the
band's paint, the greeter and matugen (`color hex` on it) all read that.
`wallpaper.animate` (on under `cartridge`) puts the shard sculpture
(`fs-chrome/src/shards.rs`, rasterised by `shards.rs`) on a Background
layer of its own over each output's wallpaper at half its pixels, with the
grid moved out of the picture onto a third layer over it; the
`backdrop.shards` and `backdrop.mark` roles colour it, and the lock draws
the frame it stopped on under its grid.
Hyprland follows through `formalshell-chrome.lua` (`rounding`, `blur`, and
the `window` role's own gaps, frame and cast, written by
`fs-theme/src/chrome.rs`), published beside the colours; mango through
`formalshell-chrome.conf` and `formalshell-colors.conf` in
`~/.config/mango`, the same window role as its own options with the palette
roles already resolved. A surface reads
the radius ladder, the font families, the icon set, the dither flag and
`Theme::pill_radius`, and never the preset's name.

## 2. Primitives

`formalshell-rs/src/ui/` holds one widget per shadcn part the shell uses.
A surface describes its content as an `El` tree (`formalshell-rs/src/ui/el.rs`) built with the
constructors in `formalshell-rs/src/ui/w.rs`, and `Ui` (`formalshell-rs/src/ui/mod.rs`) keeps each element's
nodes and running tweens between draws. Build with these; add a primitive
only when two surfaces need the same new thing.

| primitive | is | states |
| --- | --- | --- |
| box (`ui::boxes::paint`) | the one chrome renderer: draws its role's box from `Theme::box_style`, in order the casts (blurred layers, under everything), the rings (filled bands at a negative margin, under the fill), the fill with its border, the face gradient, the inset hairlines, the pointer's wash, then its content; fill and border cross on `Cx::color` | whatever states its role declares; an unknown state reads as the role's base |
| cell (`w::cell`, `w::list_row`) | the `cell` role at `radiusMd`: bar cell, list row, chip; a cell under a list that owns the cursor or the selection fill draws neither itself | `CellState`: rest, `ghost` (the bar's own cells), hover, `active`, `selected`, `destructive`, `warning`, `chip`, cursor (the `cursor` role composed over it) |
| button (`w::button`) | the `button.<variant>` role, `Variant` one of `Default`, `Outline`, `Ghost`, `Selected`, `Destructive` | rest, hover, press, cursor, disabled (opacity 0.5, off the table) |
| icon button (`w::icon_button`) | a ghost button that is `controlHeight` square, one icon | as button |
| card (`surfaces::card::Card`) | the `card` role at `panelPadding`, opaque on a surface the compositor does not blur; the surface's own frame, never nested | rest, opaque |
| shoulders (`formalshell-rs/src/surfaces/shoulders.rs`) | the `card` role with the anchored edge left open and a concave fillet outside each of its two corners, running out to the line it came out of (§1 Motion), and, at `attach` 0, a plain card again: what every surface coming out of a line draws | attached, letting go, free |
| drawer (`Card`, over `fs-chrome/src/drawer.rs`) | the edge-anchored card, two recipes picked by the live theme's `emerge` habit and never a theme name (§1 Motion, M57 D4, M60 T2): `join` (the metamorphosis) derives the line, the depth, the walls, the nested bud's span and the deform's two pivots, and runs the pose, the let-go (`Card::attach`), the deform and the shoulders; `popover` (elementary's) drops the card straight out of the cell that opened it, with no line, no gap and no deform. Panel, the notification centre, the launcher, polkit, the radio atlas and the OSD all sit on it | attached, letting go, free, bypass (the pose lands at once, for a handoff) |
| picture (`w::picture`) | content imagery, bare: the retro pass under `theme.dither`, no frame and no rounding | none |
| cover (`w::cover`) | a picture in a `muted` well with a 1px `border`, clipped to `Theme::cover_radius`: album art, a notification's app icon | none |
| section label (`w::section_label`, `w::section`) | `caption`, `medium`, `mutedForeground`, sentence case; optional trailing count `(3)` | none |
| input (`w::input`) | the `input` role at `radiusMd`, `controlHeight`, placeholder `mutedForeground`; a dragged selection paints the `input.selection` role behind the text | rest, focus (ring), error (caption below) |
| switch (`w::switch`) | a 32x18 `switch.track` (`off`/`on`) holding a `switch.knob` | cursor (ring) |
| button group (`w::group`) | a `trough` role at `radiusMd` holding one ghost button per option, `xs` inside: a choice among several (power profiles, the audio device pick) when exclusive, a set of actions (the media transport) when not | selected (`button.selected`), active option (`primary` fill), cursor (ring on one button) |
| segmented (`w::segmented`) | a `trough` role, `radiusMd`, its `segmented.chip` fill travelling to the active segment on `spatial` | hover on an unchosen segment (the wash, ink lifted off `mutedForeground`), cursor (ring) |
| track (`w::track`, `w::slider`) | a `trackThickness` progress or slider: `track.groove`, `track.fill`, `track.notch` for the one mark it can carry | cursor (ring), for a surface that addresses the track as a row |
| separator (`w::separator`) | a 1px `border` rule, full-bleed, inset or vertical | none |
| power flow (`w::flow`, `formalshell-rs/src/ui/draw/flow.rs`) | the Power panel's diagram (each node an icon, caption, mono value and detail, flat; each link a separator rule): adapter, laptop and battery in a row and a trunk with one branch per USB-C port. A link carries a chevron only while power crosses it; the chevron travels on the charging pulse's clock (the continuous-motion carve-out) and rests mid-line under `motion.enabled: false`. Figures come from `formalshell-rs/src/services/info/powerflow.rs` and a reading the kernel does not give is left out | none |
| tooltip (`formalshell-rs/src/surfaces/tooltip.rs`) | the `popover` role at `radiusSm`, `caption`, 6px off the anchor; one surface per output | delayed (400ms), travelling (within 500ms of the last hide) |
| keys (`Host::key`, `fs-chrome/src/cursor.rs`) | key dispatch for keyboard-driven surfaces (Escape, Tab, arrows and hjkl, Enter, Space, x, printable) | blocked while an inline editor has focus (`cursor::catcher_blocked`) |
| tween (`Cx::tween`, `motion::Animated`) | the one number animation in the shell: `motion::Kind` resolves a duration and a curve out of the motion tokens (§1 Motion), `spatial` by default | none |
| colour tween (`Cx::color`) | the colour half of it, always `effectsSlow`: every fill and border that changes colour | none |
| deform (`motion::Deform`) | the velocity squash (§1 Motion): samples the card's live rect each frame and hands its matrix to the card's transform, an amount per surface | running, at rest (identity, no frame requested) |
| scrim (`card::Scrim`) | the modal backdrop (§1 Motion): the `scrim` role's own colour on a drawer's own pose, its share over the line's own band riding `1 - attach` so a card buds off a lit bar and dims it only as it lets go | attached, letting go, free |
| panel (`formalshell-rs/src/surfaces/panel/host.rs`, the `Panel` trait in `formalshell-rs/src/surfaces/panel/mod.rs`) | the popout window: a drawer under a bar cell, header row (icon, title, icon buttons), key dispatch around the content, one travelling cursor ring and the scroll that follows it, the frame's size and position morphs | open, closed, handing over |

## 3. Surface rules

**Bar.** One continuous strip along one edge of the output (`bar.position`,
top by default): `card` fill at `surfaceOpacity`, a 1px `border` along its
inner edge and no other edge, `barCellHeight + 2 * barMargin` thick on a
top or bottom bar and `barCellWidth + 2 * barMargin` on a left or right
one, no margin on the edge it sits on. Regions inset `md` from both ends of
the strip; cells sit `barMargin` in from the outer edge and are the strip's
own cell thickness, `sm` apart, grouped where Omarchy groups (workspaces,
indicators). The layout resolver is `fs-chrome/src/bar/layout.rs`; the
strip and its cells are `formalshell-rs/src/surfaces/bar/`.
On a left or right bar the same three regions run top to bottom (`left` at
the top) and nothing turns: each cell stacks its icon over its label
upright, which is why the strip is wider than a horizontal one is tall. A
label too wide for the strip wraps into it and stands down only when a
single word still does not fit, leaving the icon and the tooltip to carry
the cell. The two exceptions turn because they cannot do either: a window
title and a now-playing track are free text of no fixed length, so those
alone rotate, reading bottom to top on the left and top to bottom on the
right. The open-panel mark and the tooltip go to the side facing the
desktop, and a panel hangs `barMargin` off that side at the cell that
opened it. Cells are ghost cells: no fill and no border at rest, since
the strip already carries both. Hover, cursor, active, selected,
destructive and warning draw as they do anywhere else. A cell whose panel is
open draws a 2px `primary` line along its bottom edge.

The workspace cell is Spaces (M74, after omarchy-spaces): a group of ghost
cells the way the indicators are, one per workspace, each its ordinal
(mono, never the compositor's name for it) followed by the icons of its
windows in screen order, through the same app-icon chain the switcher uses.
Each is a bar cell like any other: its own padding, the strip's thickness,
`sm` to the next, no fill at rest, the `cell` hover wash under the pointer,
and its icons the height of the number's line, `xs` apart, as the window
title's app icon is sized to its name. Workspaces 1 to `workspaces.persistent`
always show, empty or not, and every occupied one shows its icons
(`workspaces.showApps`: `all` by default, `active` or `hover` to narrow it).
The focused workspace is the pill: the `cell` role's `active` fill as one
item that travels on a switch (§1 Motion), stretching over that cell's
number and icons, with `primaryForeground` ink on it; there the windows not
holding focus are dimmed. Every other number is `dimForeground`, lifted to
`foreground` under the pointer, so on a band both follow its paint. An
urgent workspace takes a `destructive` number that pulses once. Past
`workspaces.maxIcons` the rest are a `+n` count. A window whose herdr agent
is working, waiting or done carries a badge on its icon (a spinner, a
pulsing alert in `destructive`, a check in `primary`), and a workspace with
an agent waiting on it breathes its number in `destructive` until it is
looked at. Clicking a cell goes there, clicking an icon focuses that window,
and the wheel steps workspaces. Resting on another occupied cell for the
tooltip's delay hangs a panel off it, joined like any other and marking its
cell open: the panel header names the workspace and its window count, over
a miniature of the output with each window drawn live at its own place (a
capture of its toplevel, rounded like any picture, its icon in the
corner), and a footer naming the window under the pointer. A window the
compositor sends no frame for is drawn as its schematic cell instead. The
card takes no keyboard when the pointer opened it, follows the pointer
between cells and closes once the pointer is on neither. `workspaces peek
<n>` opens the same card over IPC, as an ordinary panel. The miniature is
`formalshell-rs/src/surfaces/panel/workspace_preview.rs` over the strip
widget in `formalshell-rs/src/ui/strip.rs`, its window and captures `formalshell-rs/src/wayland/preview.rs`.

Window capture has two owners and no others: this preview and the Alt+Tab
switcher (`formalshell-rs/src/surfaces/switcher/`, `formalshell-rs/src/wayland/switcher.rs`).
Both take thumbnails through `formalshell-rs/src/wayland/capture.rs`
(ext-image-copy-capture on each window's foreign-toplevel handle, keyed by
`capture::Owner`), and only while their card is open: closing the card drops
its owner's set, which destroys every frame, session and source it held.
The lock surface and everything else never capture a window.

The tray's place on the bar is a dots toggle, and the icons themselves live
in a second bar hanging off it. The strip can carry them instead
(`tray.maxVisible`: -1 for as many as fit, N for up to N), but never some here
and the rest there: the tray moves whole, so it never reads across two
surfaces and the cut never moves under the user when something else on the bar
resizes. Room has the last word over any ceiling, since the tray is the one
region cell with no fixed number of items and so the one that gives ground
when the strip runs out of edge. That second bar is a card of the same ghost
cells at `panelPadding`, one item deep, sitting where any panel would; it is
the tray's own overflow, and the chevron has one of its own: everything on a
chevron's governed side lives in the same kind of second bar, always, and the
chevron cell opens and closes it. It is a fixed home rather than a state a
crowded bar falls into, the same call the tray's dots take: the group is
somewhere else rather than sometimes here and sometimes there, so nothing on
the strip moves when a track starts playing.

A crowded strip gives up room in a fixed order rather than clipping a cell
in half. The two free-running labels, the now-playing track and the window
title, share what the cells leave, and each scrolls once its text outgrows
its share. A playing track keeps a readable minimum (about twenty
characters) and the title a shorter one, both only out of room the cells
leave: on a strip too short for both, each is first sure of an even share,
then the track's minimum fills before the title's grows. Past the
minimums the two split the rest evenly, a label that
needs less handing its surplus to the other, up to where the centre still
sits at the middle of the strip. The window title never grows past a
ceiling of its own: the whole cell stops at `popupWidthNarrow`, about
thirty characters of title beside a short app name, however much room the
strip has. Once an end region's cells alone push the centre off the
middle, the middle stops bounding anything and the strip does. Past that,
what still does not fit hides
whole cells from an end region's own inner edge, the one nearest the
centre, never the one against the screen edge; a cell comes back the
instant the room does. The chevron takes no part in any of this: it stays
config-only, collapsing whatever bar.layout put on its governed side
whether the strip is crowded or not. Along the strip only the cells
themselves animate their size; the regions, their clips and the centre's
position follow those sizes frame by frame with no motion of their own,
so no two cells ever sit on top of each other mid-glide.

**Wingpanel band** (`bar.kind: wingpanel`, the pantheon habit). No strip
card, no hairline, no cell borders: the band's paint is read off the
wallpaper under it rather than drawn as chrome, one of five paints
(`light`, `dark`, `translucentLight`, `translucentDark`, `maximized`)
decided by the band's own mean luminance, standard deviation and acutance
against wingpanel's own thresholds (mean 180, spread 45, acutance 8, plus
the 1.645-sigma rule for a band whose mean falls short but whose spread
puts a twentieth of it past 180) and Hyprland's fullscreen state, which
forces `maximized` outright (`fs-theme/src/barpaint.rs`, sampled by
`formalshell-rs/src/services/barpaint.rs`). Each paint carries its own ink
and elementary's own text shadow under it, both layers of it: the table's
`inkShadow` is a CSS layer list (`{ x, y, blur, color, alpha }`, first on
top) and the bar cell's painter (`Painter::text_in` in
`formalshell-rs/src/surfaces/bar/cell.rs`) draws one blurred copy of the
text per layer under it, so the blurred half is rendered rather than
approximated. Black at 0.3 and 0.6 under white words on a bare band, 0.15
and 0.3 once the band has a fill of its own, and none at all under dark
words or over white at 0.5. `bar.paint` in settings.json overrules the
reading: `transparent` keeps the ink adaptive and drops the fill, and the
five names pin one paint outright. `bar paint` and `debug dump` report the
paint, whether it was pinned, the output it was read on, and the three
numbers.

One reading, not one per monitor: sampling each panel against its own
screen left a two-output desk with one bar in dark ink and the other in
white. The sampler runs on the main display alone
(`display.outputPriority`) and every band and frame ring wears its answer.
A window covering an output is the one term that stays that output's own.

The ink goes where the band does and no further. A cell in the chevron's
second bar or the tray's is on a popover card with a fill of its own, so it
resolves its ink from its state against that card, never off the band's
reading of a wallpaper it is not drawn on.

On a bare band (the `light` and `dark` paints, under the rule or
`transparent`) each cell reads its own ink off the stretch of wallpaper
under it rather than wearing the band's: the sampler keeps the band as a
profile along its length (`barpaint::Profile`), the bar pools the columns
under each placed cell's rect and `barpaint::cell` applies the mean rule
to that alone, so a wallpaper white at one end and dark at the other gets
dark words over the white and light ones over the dark, with the `light`
paint's shadow under them, instead of one ink that reads on half the
strip. The band itself still decides the fill: a band busy enough for a
translucent paint, a pinned paint and a covered output keep one ink
across, since the fill is what the words sit on. `bar paint`'s `cells`
lists what each placed cell settled on.

**Frame.** A theme habit first (`habits.frame`): a table that says it wears
no ring reads `frame.thickness` as 0 whatever settings.json holds, so the
key reserves nothing and the `window` role's own gaps are the whole margin
round a window. metamorphosis and retro frame, pantheon does not. Off by default
(`frame.thickness` 0). On, the bar's own fill continues round the other
three edges as a band `frame.thickness` wide, and
a rounded rectangle (`frame.radius`, 20; 0 with a base radius of 0) is cut
out of the whole for the desktop, so the bar reads as the thick side of one
frame and the two corners beside it curve into the strip
(`fs-chrome/src/frame.rs`). The bar's window
grows to the output and paints the whole ring, strip included, then its
cells over it, so the three are one surface under the compositor's blur and
above windows; it draws the single 1px `border` along the cut-out and takes
input on the strip alone. Under the wingpanel habit the ring takes the
band's own paint from the same sampler (M62): the bar's band is a stretch
of the ring there, so a calm wallpaper leaves the whole frame undrawn, a
busy one washes it, and what wingpanel puts along its panel's inside edge
lands on the ring's hairline. Its cast is dropped, having only the desktop
inside the cut-out to fall on. Windows tile
inside it (the band is an exclusive zone on each of its edges), and every
floating surface clears it the way it clears the bar.

**Which primitive.** An on/off state is a switch, never a button whose
label is its own state and never an icon that flips between an on and an
off name. A choice among several, or a row of actions belonging to one
thing, is a button group. A level is a track. Text entry is an input. A
badge inside a row is a chip cell. A heading is a section label. A seam is
a separator. A surface that paints its own bordered or rounded box is
drawing chrome a primitive owns; the only shapes a surface draws outside
the `ui::w` widgets are the ones no primitive covers (an indicator dot
`w::dot`, a colour swatch `w::swatch`, a QR module `w::matrix`).

**Panel.** Header, then sections. A panel is ONE card. A section is a
section label and a column of cell rows `rowGap` apart, and almost nothing
in one draws a box at rest (owner, 2026-08-26, "there's a too big usage of
cards everywhere"). A repeating list row in a section where every row is
interactive is a ghost cell with no gap between rows, the shape the tray
menu and the notification centre already had: uniform rows need no border
to read as controls, the hover wash and the cursor ring say it. A block
that only reports a number, and an empty state that only says
`NO DEVICES`, is a ghost or no cell at all. What keeps its `radiusMd` fill
and border is what a reader could otherwise mistake: a lone control that is
not part of a list, and every row of a section that mixes clickable rows
with static ones. Every state still draws on a ghost, so nothing is lost
but the resting box. The header takes a full-bleed separator under it, on
every panel, so the card reads as a titled sheet rather than as a title
floating over a list. A hero (`w::hero`: the connected AP, the active sink,
the playing track) is flat and leads the content column, its own type doing
the ranking the border used to do: a `subtitle` title over a `bodySmall`
caption, an optional `display` readout beside them, an optional track
under them. Footer: `outline` button left, `display` number right. Width
`Default`; `Wide` for media, monitor, calendar. Nothing in a panel scrolls
except a row list longer than the screen. The media panel spends its one
card (§1's ladder, rung 5) on a lyrics pane (`formalshell-rs/src/ui/lyrics.rs`)
trailing the now-playing column on the same gutter and split M55 set
(M56 P13), `radiusMd` and a `card` fill with nothing else drawn inside it:
the pane's viewport rests its anchor line 42% down rather than at centre
(kopuz's comfort offset, M56 P9) and travels to meet it, and a duet turn
or a background vocal overlapping its parent can light more than one line
at once (M56 P5). Every other line still carries M55's opacity ramp on
its distance from the anchor, and, with `media.lyricsBlur` on, a blur
growing with that same distance and capped at 6px, the one exception to
this file's no-blur rule above (M56 P7). The pane is never shorter than
six rows, following the now-playing column's own height above that. A lit
line's chunks wipe with a soft band sliding across a `mutedForeground`
copy under a `foreground` one, the chunk being sung carrying a glow that
decays once it ends, whether or not the source gave word timing: an
untimed line gets its own chunks synthesised so the wipe never fails to
show (M56 P4/P6, replacing M55 A3b's hard clip). A silence draws a `music`
icon the same way, its lit copy clipped left to right over the gap (M55
A4). A wheel over the pane takes the scroll over from the song, clamped to
its own ends, until a `refresh-cw` ghost control at the pane's bottom
right, a new track, or the keyboard cursor hands it back (M56 P9). The
now-playing column itself runs horizontal (M55 A2): the elapsed time, the
track and the total sit on one line, and the transport and the player's
own volume share another, transport leading. Its spectrum
(`formalshell-rs/src/ui/spectrum.rs`) sits inline at the trailing end of
the identity row, beside the title rather than under it: twelve columns of
the same `muted` trough and bottom-up fill the bar cell draws, coloured by
the same three energy bands off the one shared cava process, carried toward
every new frame rather than snapped to it, so the motion runs at the
screen's own refresh rate.

**Popover emerge** (`emerge: "popover"`, the pantheon habit). A panel, the
launcher, the OSD, polkit, the notification centre and the radio atlas all
read this off the drawer the way they read the joined recipe (§1 Motion,
§2 drawer): the card drops straight out of the cell that opened it on
Gala's 150ms menu map instead of budding off the bar's line, so there is no
let-go, no gap published to the line, and no shoulders either: the card
is a plain rounded rectangle on all four sides.

**Launcher** (`formalshell-rs/src/surfaces/launcher.rs`). shadcn Command:
a `Menu`-wide card at 30% from the top; input with a bottom rule only; a
shadcn Breadcrumb under it (ancestors in `mutedForeground`, the level in
`foreground`, a `chevron-right` between, no fill and no frame); rows with
the cursor row in `accent`; hint footer in `caption` `mutedForeground`.
Modal over a 0.5 black scrim. The split route's preview pane is the one
card this surface spends inside its own frame (§1's ladder, rung 5):
`radiusMd`, an `sm` gutter off the list, flat rows beside it. Nothing
inside the pane draws a frame of its own, the preview picture included.
`menu.appGrid` defaults true under every theme (M72 T2), swapping app rows
for Slingshot's icon grid; a user `false` still wins. A route is where you
pick from a list (a network, a sink, a station) and a panel is where you
watch live state, so a device that has both keeps both, and the launcher
row never grows the panel's controls.

**Toasts** (`formalshell-rs/src/surfaces/toasts.rs`). The sonner stack:
collapsed, the front card with two levels behind it, each one the whole
card scaled down by `lg` per side (95% and 90% of a 320 card) about the
edge its sliver shows from, as tall as the front card and `panelPadding`
further from the anchored edge, so the sliver is the same whatever the
card behind holds; deeper cards wait unseen behind the last level.
Hovering fans it out into the plain column `panelPadding` apart at every
card's own height, in the pile's own order with the front card still
against the anchored edge, so the card in front never moves and nothing
trades places. Card
chrome; critical is a `destructive` border and icon, not a
fill. The card's icon slot resolves the notification's image, its app icon,
the sender's desktop entry, then a `bell`; a picture takes a `radiusSm`
frame. The surface under the stack is the whole output and holds that size
for as long as it is mapped, so a compositor's own layer animation has no
geometry change to fight; the cards move, and everything outside them is
click-through.

**Bubble** (`notification: "bubble"`, the pantheon habit). elementary's own
notification, drawn by `bubble` in the same `toasts.rs` that draws the
toast above: 332 wide, radius 9, icon left with a 6px gap, bold title, body
wrapped at 33 characters, a round close button that appears on hover. It
flips in over 400ms (opacity plus an x rotation from 90 degrees through -10
at 60% to 0) and restacks on `emphasizedDecel` 200ms with a 150ms stagger
across however many bubbles move.

**Notification centre** (`formalshell-rs/src/surfaces/panel/center.rs`). A
floating card off the right edge, content-tall and capped at the output;
DND is a switch in a ruled header; unread rows carry a 6px `primary` dot.
Its rows are multi-line, so a separator runs between them (§1's ladder,
rung 4) and the two tiers stay labelled sections `sectionGap` apart.

**OSD** (`formalshell-rs/src/surfaces/osd.rs`). A pill bottom-centre,
budding off the bottom line: icon, track, tabular percentage.

**Lock, greeter** (`formalshell-rs/src/surfaces/lock.rs`,
`formalshell-rs/src/greeter/`). Wallpaper, 0.5 scrim, `displayLarge` x3
clock, date as a section label, one input. Wrong password: the input's
error state. The lock screen puts the profile picture (`avatar.path`,
default `~/.face`) over the clock as a circular avatar the clock's height;
no readable image, no slot. The greeter draws none: it has no account until
a username is typed. The lock's words sit on the wallpaper and take the
`lock.ink` role's `light` or `dark` state by contrast with the wallpaper
under their own rect, scrim included, never by the palette's mode. Under
the field, while something is playing or paused, the now-playing block: a
cover, title and artist in that ink, a track, and the transport as a button
group the field's own Tab reaches without the field ever losing focus.

**Picker.** Thumbnail cells with `radiusMd`; cursor is the ring;
`Dark | Light` is a segmented control.

**Tooltip, tray menu, polkit, console, capture, hot corners.** Same tokens,
no exceptions.

## 4. Keyboard

Anything a pointer can do on a shell surface has a key, and the target is
visible. Panels take keys through the panel host's key dispatch and show
the cursor as the ring; the launcher shows it as the `accent` row. A panel
opened by pointer hides the cursor until the first key, and a pointer
moving the cursor takes the ring off it again (§1 "Ring"). `panel toggle
<name>`, `panel toggleAt <n>` and `menu summon <route>` are the keybind
entry points; the shipped bindings are in
`docs/examples/hyprland/formalshell.lua` and
`docs/examples/mango/formalshell.conf`.

## 5. Never

- A literal colour, radius, duration or pixel size in a surface module or a
  primitive.
- A colour role, `Theme::surface()` or a radius step read for chrome
  anywhere but a theme table (`fs-theme/themes/`). A primitive reads its
  role and state through `Theme::box_style()`; ink stays a token, since it
  is content rather than chrome.
- A duration or a curve outside `motion::Kind` and the colour tween. The
  workspace pill's trailing edge, which doubles the `emphasized` token on
  the cell, is the one carve-out: the relation between its two edges is the
  effect, and a second token would be a name with one caller.
- A hardcoded font family, a Nerd Font glyph, an SVG icon asset.
- Words in mono or values in sans.
- A shadow, a gradient, or a blur drawn by the shell (blur is the
  compositor's, behind a translucent card), save the lyrics pane's depth
  of field and its sung-chunk glow (owner, 2026-09-17), and the cartridge
  table's bracket glow (its own spec, 2026-10-23). Dither only behind
  `wallpaper.dither` or `lock.dither`, both off by default. The bar's own
  mini cover stops animating behind `media.animatedBarCover: false`; the
  media panel's own cover animates unconditionally.
- A window capture outside the Spaces preview and the Alt+Tab switcher, or
  one held while their card is closed.
- A card inside a card, and a resting fill-and-border around any block that
  only groups its children or only reports a value.
- Foreground/background inversion for selection.
- A trailing colon on a label. `Networks (1)`, not `Network:`.
- A full-bleed `primary` or `destructive` row. Colour goes on the border, the
  icon or the dot; fills are for buttons and the active toggle.
- A hover fill that moves, or a cursor that slides between rows.
- Uppercase anywhere. Headings are sentence case (2026-09-18 theme
  boundary), same as the breadcrumb, which lost its own carve-out with its
  chips (2026-08-26): shadcn's breadcrumb is a path in natural case. A real
  acronym (CPU, IP, SSID) or an identifier handed in verbatim (`eDP-2`, a
  device name) keeps its own case.

## 6. Verify

`just vm-smoke <flag>` on nested Hyprland, then read the PNG. A change to a
token or primitive re-runs base, `--menu`, `--notify` and `--panel network`
at minimum; a change to one surface re-runs that surface's leg. Contrast:
`mutedForeground` on `card` stays at or above 4.5:1 in both fallback modes.
A theme table change runs fs-theme's tests first (every role in every
table resolves against a palette whose roles answer with their own names,
`fs-theme/src/style.rs`), then the legs whose frames it touches, read by
eye. The pantheon habits add their own legs: `--pantheon` rides any other
for the table alone, `--bar-adaptive` reads the wingpanel band's paint off
four fixture wallpapers, and `--notify-emerge` reads the bubble's flip-in
frame by frame.
