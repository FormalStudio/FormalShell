# The cartridge theme

Owner, 2026-10-23: "a formalshell theme that basically replicates
https://tsubi.club/. Its interactivity, delightful details and aesthetics
are awesome. It feels like an old console game in a way." Then: "don't
just call it tsubi. It must be as well animated and interactive, and the
panels need to be reimagined in this theme." Then: "tsubi has an animated
background, not a static image, so that's special for this mode. And they
also have an entire UI aesthetic and a boot screen type thing that's
nicely animated and transitions."

So `cartridge` is the fourth preset, and the first theme allowed past
`2026-09-18-theme-boundary.md`: the owner asked for its panels, its
backdrop and its session start to be reimagined, not restyled. Where this
spec and the boundary spec disagree, this spec wins for `cartridge` only;
every other table stays inside the boundary. Each widening is a habit or a
role every table declares (metamorphosis, retro and pantheon take the
existing behaviour), never a surface testing a theme name.

Owner's choices on 2026-10-23: original sound effects, palette from
matugen, packaged fonts with an animated width axis, a grid on the
backdrop.

## The reference

Read off tsubi.club at 1440x900 on 2026-10-23: its five pages, its CSS
(every keyframe and transition), and frame bursts of the boot and the
backdrop. Frames are in `artifacts/ref/` (gitignored: the site's
art is not ours to commit). Nothing of theirs ships: no audio, image,
wordmark or font file of theirs.

**Material.** Flat and dark. Page `rgb(24 26 27)`, ink `rgb(232 230
227)`, charcoal chips, 1px outlines in the section's accent, insets
rather than casts, small radii, chamfered corners on framed boxes, a
narrow vertical label strip on the side of a panel ("updates").

**Type.** Archivo (variable, wdth 62..125, wght 100..900) wide and in
caps for navigation and headings; a condensed caps face for `//
SUBTITLE`; BIZ UD Mincho for body copy; pixel faces (DotGothic16) for
values and status lines.

**Section grammar.** Every inner page is the same frame: a wordmark and
`// SUBTITLE` in the accent with a soft glow, a "Go Back" chip, a pill
outlined status strip reading like a memory card browser (`MODULE 1 ::
2932 blocks free` on the left, `( 2026 / 10 / 23 )` on the right), then
the content. Each section owns one accent hue (teal home, orange files
and network, violet blog and shop). Field labels (`TITLE`, `TAGS`,
`INFORMATION`) sit on a short accent bar fading to the right. A preview
box is framed with a tab label (`*asset.viewer`) and one chamfered
corner. Tags are small outlined pills. The primary action is a wide pill
with a hexagonal end cap holding its icon.

**Selection.** In a list, a solid block bar on the item's leading edge
slides between items and the item's fill fades in. In a grid, tiles are
hexagons and the selected one is held by bracket arcs just outside it,
with a glow in the accent.

**Motion.** Two easings carry everything: `cubic-bezier(0.22, 1, 0.36,
1)` (200ms for hover and focus, 500ms for colour and the cursor) and a
spring with real overshoot (a CSS `linear()` peaking at 1.42, settling
over 1.1 to 1.3s) for size changes. The catalogue:

| Site effect | What it does |
| --- | --- |
| panelAppear | a panel slides 40px in from the side and fades, 0.8 to 1s |
| liAppear | list items rise 40px and fade, staggered, 0.9s |
| typing | a value types itself out, `steps(20)` over 0.6s |
| tagAppear | a tag slides 10px in with a red glow pulsing out |
| appear (glitch) | content jumps 10px back and vanishes, reappears 20px ahead and settles |
| use | on activate, a ghost of the item scales to (1.1, 1.3) and fades, 0.4s |
| shake (text) | on error, weight, width and tracking jitter over 0.5s |
| shake (icon) | an icon wiggles plus and minus 5 degrees |
| viewerAppear | a picture lands from scale 20 to 1 in `steps(2)` with blur and brightness |
| noiseFade | a noise layer over a fresh page fades out over 1s |
| flicker | a CRT flicker at 0.07 to 0.13 opacity, forever |
| loading-bar | `█████░░░░░ 50%` text blocks as a progress readout |
| slam-in | the intro's items slam in from 6vw to the left |
| slide | the grid drifts one cell diagonally every 8s, forever |
| heading hover | width, weight and tracking widen over 200ms |

**Sound.** Every interaction has one: `nav_hover`, `interact`, `back`,
`new_page`, `typing`, `misc`, `addtocart`, plus a music bed.

**Boot.** Black. Grid lines grow in from the edges at different speeds,
an emblem ring spins in from edge-on, a stacked wide wordmark assembles
inside it, the screen waits. On a click the emblem crumples and the
teal world fades up under it: grid, a giant faint watermark mark, the
backdrop, the navigation slamming in.

**Backdrop.** A three.js scene: a low-poly sculpture of shards, flat
fill in the page colour with white edge highlights, turning slowly in
front of a faint giant mark and the grid. A huge letterform beside the
navigation changes with the hovered item.

## The bar

Owner, 2026-10-23: the polish has to match the site's.
`2026-10-23-cartridge-theme/preview.html` (published as a claude.ai
artifact) is a browser mock of the target that the owner signed off as
the reference: layout, motion and timing are read off it, and it is the
floor, not the ceiling. Where the mock and the site differ in finish,
the site wins.

## Decisions

**C1 Palette.** matugen off the wallpaper, as for every theme. The table
maps tsubi's material onto the palette's roles. The per-section accent
is the palette's `primary`, `secondary` and `tertiary`, assigned to
panels by a fixed global map (each panel declares one; the other themes
ignore it). A flat teal wallpaper reproduces the site's home page.

**C2 Fonts and axes.** A table may name its families: `fonts.words`,
`fonts.values`, `fonts.display` (headings, titles) and `fonts.body`
(secondary lines), each falling back down that list to the generic
alias. `TextStyle` carries `wdth` and `wght` as variation axes. Archivo,
Archivo Narrow (the condensed caps), BIZ UD Mincho and DotGothic16 are
OFL and ship with the shell (flake and tarball), registered through
`FS_RS_FONT_DIRS`.

**C3 Curves.** A table curve may be a list of `[t, v]` stops, CSS
`linear()`'s piecewise form, beside the existing names and bezier
control points. `cartridge` uses it for the spring.

**C4 Habits and roles.** New, declared in every table:

- `emerge: "slide"`: the card slides 40px along its anchor line and
  fades, on the table's emerge clock. Joined shapes do not apply.
- `panel: "sheet" | "console"`: `sheet` is today's header. `console`
  is the section grammar: the panel's title in `display` caps and a `//
  subtitle` in the accent with a glow, a back chip (Escape's keycap
  and "Back") in place of the close button, the status strip (`<ID> ::
  <status>` and `( yyyy / mm / dd )`), field tag section labels, and a
  vertical label strip on the card's leading side carrying the panel id.
  `Panel` gains `subtitle()`, `status()` and `accent()`, implemented by
  every panel; only `console` draws them.
- `grid: "square" | "hex"`: launcher grids (apps, emoji, wallpapers)
  draw hexagonal tiles with the bracket selector.
- Roles: `cursor` gains `bar` (a leading edge block of a given width)
  and `brackets` (arcs outside the box); `frame.chamfer` (corner cut
  size per corner) on boxes; `hero.tab` (the tab label over a framed
  hero); `button.default.cap` (the hexagonal end cap).

**C5 Motion.** A table's `motion` gains, all optional, absent meaning
today's behaviour:

- `focusText`: the cursor row's label goes to a width, weight and
  tracking, 200ms on the default ease. A heading the pointer hovers does
  the same.
- `reveal: "type"`: a hero title, the status strip and a value that
  changes type in by characters, `steps(20)` over 0.6s, the full string
  measured first so nothing reflows.
- `arrive.stagger` (exists) at 40px rise for rows on open.
- `swap: "glitch"`: a body whose content is replaced (a panel handoff,
  a launcher level change) jumps back 10px and out, then in from 20px.
- `use`: a ghost of the activated row scales to (1.1, 1.3) and fades
  over 0.4s.
- `error: "shake"`: the field's error label jitters weight, width and
  tracking over 0.5s, the field's icon wiggles. Lock, polkit and the
  Wi-Fi passphrase are where it fires.
- `picture: "zoom"`: a cover or preview that changes lands in two steps
  from a large blurred scale.
- `noise`: a grain layer over a fresh card fades out over 1s.
- `progress: "blocks"`: progress readouts (OSD, speed test, updates,
  record) draw as `█████░░░░░ 50%` in the values face.

No effect loops at rest. The flicker and the drifting grid are cut from
the shell's chrome; the backdrop (C7) is the one thing that keeps
moving, under its own rules.

**C6 Boot.** `boot: "cartridge"` (a habit; `none` elsewhere). On the
greeter: black, the grid growing in, the emblem ring spinning in, the
user's hostname set as the stacked wide wordmark inside it, then the
password field typing its prompt in. On authentication the emblem
crumples and the session fades up. In the session, the same sequence
plays once per login as the shell's first frame over the desktop
(never on a shell restart: `state.json` records the boot id it played
for), skippable by any key, then the bar and backdrop slam in. Unlock
uses the short form: the clock slams out, the desktop fades up. The
emblem and wordmark are drawn by the shell; nothing of tsubi's.

**C7 Animated backdrop.** `wallpaper.animate` (preset default true for
`cartridge`): the shell draws a procedural shard sculpture over the
wallpaper: a seeded set of thin triangles in a slowly rotating 3D
cluster, projected and drawn flat in the palette's surface colour with
`onSurface` edges, at the wallpaper's leading side, plus a giant faint
mark and the grid (C8). Budget, measured by `--idle` riding
`--cartridge`: at most 24fps, rendered at half the output's pixels and
scaled, under 4% of one core on g815 and under 10% on e1504g's power
saver stand-in while it runs. It runs only while the backdrop is
visible: paused with zero commits under a fullscreen window, when
tiled windows cover more than 85% of the output, on a locked session
(the lock draws its own still frame of it), in power saver, on
battery under 20%, and when `theme.reducedMotion` is set. Paused, it
holds its last frame. The launcher's letterform (C9) is drawn on the
launcher card, not the backdrop.

**C7a Flat desktop.** `wallpaper.color` (preset default `#402484`, the
deep purple of tsubi.club's `--color-2`): the desktop, the lock and the
greeter show a flat field of that colour in place of the picture, the
grid and the sculpture over it, and matugen seeds the palette from it.
It takes a hex or one of tsubi's five preset names (sage `#a1c8bc`,
purple `#402484`, green `#68bc64`, silver `#bdc3c9`, orange `#ff8000`);
`false` or `null` gives the picture back. The wallpaper picker offers the
five as cells ahead of the pictures, and its choice (a colour or a
picture) lands in `state.json` and wins over the key.

**C8 Grid.** `theme.grid` (preset default), overridable by
`wallpaper.grid` and `lock.grid` like dither: lines on a fixed pitch in
logical pixels times the output scale, `onSurface` at low alpha, fading
over the bottom fifth. Drawn into the wallpaper pixels after dither,
in the lock and in the greeter. Static.

**C9 Surfaces.** What `cartridge` makes of each, all through the habits
and roles above:

- Bar: dark chips, the hovered or open cell carrying the block bar on
  its leading edge, values in the pixel face, the clock typing in on
  the minute.
- Launcher: root rows in `display` caps, widening under the cursor; a
  giant faint letterform of the cursor row's initial behind the list,
  crossfading on the 500ms clock; hex app grid; the status strip
  showing the query's result count.
- Panels: the `console` grammar, rows rising in on open, values typing
  in, the hero framed with its tab and chamfer.
- Notifications: the "updates" card: the vertical label strip, caps
  title, body in the body face, arriving with `tagAppear`'s slide and
  glow.
- OSD: the block progress readout in the values face, the icon in the
  accent.
- Lock: the grid and a still of the backdrop, the clock in `display`
  caps slamming in, the password field shaking on a wrong attempt.
- Alt+Tab switcher: hex-cornered thumbnails with the bracket selector.

**C10 Sound.** A global capability with theme-owned content. The table's
`sounds` maps events to a sound: `move`, `activate`, `back`, `open`,
`close`, `type`, `toggle`, `error`, `boot`. A table without `sounds` is
silent. `theme.sounds` (bool, preset default) and `theme.soundVolume`
(0..1, default 0.5) are settings keys. The sounds are original,
synthesised by a committed generator, embedded as small WAVs, and played
in process on a PipeWire stream on `fs-audio`'s thread (already linked,
so no new child or package). `type` is rate limited, a burst of moves
plays one blip per frame at most, and nothing plays under do-not-disturb
or while locked, except the lock's own typing, error and unlock.

## Out of scope

tsubi's music bed, its mascot cursor, the shop's hologram pedestal, the
CRT flicker and the drifting grid (both loop at rest).
