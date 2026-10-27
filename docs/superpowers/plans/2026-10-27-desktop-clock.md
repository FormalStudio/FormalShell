# Desktop clock

**Date:** 2026-10-27
**Status:** planned, on `main`
**Reference:** Nothing OS 5.0's lock screen clock faces, and the one the
owner runs on their phone (`docs/superpowers/refs/desktop-clock-pixel.png`):
big square-pixel digits whose outer corners are rounded, whose corner
pixels are cut away and which carry a thin horizontal seam part way down.
`docs/superpowers/refs/desktop-clock-nothing-os-5-faces.jpg` shows the
picker's serif face and the Gooey face (dots that melt into each other).

The wallpaper depth work (2026-10-26) put the lock clock behind the
wallpaper's subject. This puts a clock on the desktop itself, behind the
same subject, nudged a few pixels by the pointer.

## Shape

- **One layer per output.** `formalshell:desktop-clock`, a Background
  layer stacked over the wallpaper and the shards/grid layers, transparent
  everywhere but the clock's box. It is handled like `Shards`
  (`surface.rs`, `heads.rs` `sync_shards`): created and dropped by a sync
  call on `update_backdrops`, double-buffered, damage limited to the box it
  drew last plus the box it draws now, no commit at rest.
- **What it draws, in order:** the clock (and the date line), then the
  wallpaper's subject over them, cropped to the same box. The subject is the
  picture with `depth_mask` as alpha (`services/wallpaper.rs`), built on the
  pool once per picture and output size and kept for the layer's life, so a
  parallax move re-crops it and never rebuilds it. With no map, or
  `desktopClock.depth` false, the clock is simply drawn over the picture.
- **Faces.** A pure module in `fs-chrome` (`clock_face`), tested by
  `rust-tests`, turns a time string into an 8-bit coverage mask at a given
  digit height:
  - `pixel` (default): the owner's face. A grid glyph table for 0-9 and
    the colon (the date line is ordinary text, not grid glyphs). Cells
    are squares; the union is rounded so convex corners carry a radius of
    about a third of a cell and concave ones a small fillet, the corner
    cells of round digits are missing, and a seam about a tenth of a cell
    high runs across every digit at the same row. Match the reference by
    eye at the same digit height before the table is final.
  - `gooey`: the same grid drawn as dots whose fields add, thresholded, so
    neighbouring dots run together into the blobby strokes of Nothing's
    Gooey face.
  - `serif`: the fontconfig `serif` alias shaped through parley at the
    same height, the colon small and raised as in the reference.
  The mask is cached per (face, text, height, width) and rebuilt on the
  minute. Nothing in it is a font file or an SVG.
- **Ink.** `desktopClock.color`: `auto` (default) picks light or dark ink
  off the picture's mean luminance under the box, the way the bar picks its
  ink (`bar paint`); `primary` takes the palette's primary; a `#rrggbb`
  string is used as given. The date line uses the same ink at 0.8 alpha.
- **Parallax.** The layer's input region is the whole output, so it gets
  wl_pointer motion whenever the pointer is over bare desktop (over a
  window the window has it, which is also when the clock is mostly hidden).
  On Enter it sets the default cursor shape. The target offset is the
  pointer's position from the output's centre, as a fraction of half the
  output, times `desktopClock.parallax` px (default 12, 0 off); the drawn
  offset follows it on the spatial spring family in `motion.rs`, frame by
  frame on frame callbacks, and stops committing once it settles. Leave
  holds the offset where it is. Nothing polls the compositor's cursor.
- **When it hides.** The layer stays mapped but makes no commit while the
  output is fullscreen (and unmaps with the rest of the chrome under
  `fullscreen.hideChrome`), while locked and under the screensaver; the
  minute tick then only marks it dirty and it redraws once uncovered.
- **Settings** (all read, never written; global, not theme-owned, per the
  2026-09-18 boundary spec, which keeps behaviour and layout global):
  - `desktopClock.enabled` (default false)
  - `desktopClock.face`: `pixel` | `gooey` | `serif`
  - `desktopClock.size`: digit height as a fraction of the output's
    height, default 0.18
  - `desktopClock.width`: `narrow` | `wide` (cell aspect and tracking,
    Nothing's two width buttons), default `wide`
  - `desktopClock.x`, `desktopClock.y`: the box's centre as fractions of
    the output, default 0.5 and 0.3
  - `desktopClock.date` (default true), `%a %-d %b` above the digits in
    `look.sans`
  - `desktopClock.hour12` (default false)
  - `desktopClock.color`, `desktopClock.parallax`, `desktopClock.depth`
    (default true). Threshold and feather are the lock's
    (`lock.depthThreshold`, `lock.depthFeather`); `DepthCut::read` takes
    the key that switches it off.
- **Observed through** `debug dump`'s `desktopClock` block, per output:
  `enabled`, `face`, `rect`, `offset`, `target`, `ink`, `depth` (whether a
  map is in use), `commits`. No new IPC target, so `ipc-golden.jsonl`
  stays as it is.

## Out of scope

Seconds, widgets beside the clock, a picker UI for the faces, any per-theme
face table. Mango gets the layer too (it is plain layer-shell) but the rig
proves Hyprland only.

## Tasks

Each task is one subagent, sequential, verification run and read before
its commit.

1. **Faces in `fs-chrome`.** `clock_face` with the three faces' masks and
   the glyph table. Tests: every digit non-empty and inside its box, the
   seam row empty across every pixel digit, a rounded corner (the corner
   pixel of a cell's box at partial coverage), `gooey` coverage strictly
   larger than `pixel`'s dots alone at the same grid, deterministic output.
   An ignored test that writes `artifacts/desktop-clock/<face>.png` for
   `09:19` at 240px; read those PNGs beside the reference crop and adjust
   the table until the pixel face reads as the same font. Commit
   `feat(chrome): desktop clock faces`.
2. **The layer.** Settings, the layer, the subject crop, ink, date, the
   minute tick, the hide rules, the dump block. `just vm-cargo test -p
   formalshell-rs` and clippy. Commit `feat(desktop): clock on the
   wallpaper behind its subject`.
3. **Parallax.** Input region, cursor shape on Enter, a `pointer_events`
   arm for the layer, the spring, frame-paced commits. Commit
   `feat(desktop): clock follows the pointer`.
4. **Smoke leg and docs.** `dev/smoke.d/desktop_clock.sh`
   `--desktop-clock`, riding the depth leg's fixture (a seeded wallpaper
   with a `.depth/` band through the clock's box): the layer mapped on every
   output; ink in the box and the band over the digits with depth on, the
   digits over the band with `desktopClock.depth` false (settings
   retarget); each face photographed; a real pointer
   (`dev/vpointer.py`) moved to a far corner moving `offset` toward that
   corner by more than 0 and no more than `parallax`, then no commit in 3s
   at rest; a fullscreen foot window and no commit for 5s; and under
   `FS_CPU_QUOTA` the minute's redraw under 50ms. USAGE.md section, the
   leg's CLAUDE.md entry, DESIGN.md's surface list. Commit `test(desktop):
   desktop clock leg`.
5. **Ship.** `programs.formalshell.settings.desktopClock.enabled = true`
   on g815 (`~/.config/nix`), the same key in e1504g's settings in
   `system.star` (formalspec branch), bump both, rebuild g815, `fspec plan`
   then `fspec apply --scope system --host e1504g`, restart the service.
