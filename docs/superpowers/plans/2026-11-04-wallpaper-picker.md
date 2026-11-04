# Wallpaper picker

**Date:** 2026-11-04
**Status:** planned, on `main`
**Spec:** `docs/superpowers/specs/2026-11-04-wallpaper-picker.md` (wins on conflict)

Each task is one subagent, run in order, ending in its own commit pushed to
`origin/main` with the verification output read and the PNGs looked at.
Every leg runs at both scales (`just smoke` does). Pond polish Task 5 (the
current mark, `w::checked_picture`, `ui/draw.rs`'s `Kind::Picture` arm,
`launcher.rs:3768`) is in flight in the main checkout as this is written:
Task 3 starts only once `git log --oneline -i --grep 'current wallpaper\|checked_picture'`
shows it landed. Pull before each task; add keys and constructors beside
other agents' work, never rewrite theirs.

## Task 1: two slots

- `crates/formalshell-rs/src/services/state.rs:25-29, 60-64, 122-123,
  150-151, 200-203, 232-233`: `wallpaper_dark: String`, `Field::WallpaperDark`,
  read and written as `wallpaperDark` (default empty).
- `state.rs:350-365`: `set_wallpaper` takes a `Choice` (`Both(path)`,
  `Pair(light, dark)`, `Slot(mode, path)`) and an optional mode, the rules
  of spec §3's table (the light slot's copy into `wallpaperDark` first; a
  slot pick under a colour or the sky setting both). Unit tests beside
  `:597-610` for every row of that table.
- `services/wallpaper.rs:41-53`: `source` takes the dark slot and the mode;
  tests beside `:378-389` (dark mode with and without a dark slot, a colour
  over both). Callers: `store.rs:139-143`, `wayland.rs:2844-2847`,
  `greeter/mod.rs:195-213` (the mirrored single file, an empty dark slot).
- `ipc/theme.rs:24-31`: `wallpaper pair`, `wallpaper setFor`, `get` the
  shown picture; `theme status` (`ipc/theme.rs:64`) gains `wallpaperDark`.
  `ipc/golden.rs:67-72` stubs and lines beside
  `crates/formalshell-rs/tests/ipc-golden.jsonl:168-175`, argument errors
  included.
- The greeter mirror in the owner's spec
  (`~/.config/formalspec/modules/linux/rootfs/usr/local/libexec/formalshell-greeter-mirror`,
  the `wallpaper=` line): the `jq` of spec §3. Commit and push it there
  (FormalSnake/dotfiles); it reaches the hosts with Task 6's apply.
- New leg `dev/smoke.d/wallpaper_pair.sh` `--wallpaper-pair`: two
  generated 960x540 pictures of known mean (a light and a dark field with
  a band each), `wallpaper pair`; `theme status`'s `desktop` and
  `wallpaperDark`; `theme mode toggle` under `debug motionScale 10` with a
  grim burst through the fade, read as spec §8 says, and back; theme.json's
  `primary` differing between the halves; `lock lock` with the lock's
  frame carrying the dark half's band, then the real password; then
  settings.json retargeted to `theme.mode: "auto"` and `desktop` matching
  `effective`. Frames dark and light pulled.
- `dev/smoke-map.toml`: the wallpaper and theme rule (`:575-576`) and the
  state rule (`:439-440`) gain `--wallpaper-pair`; `dev/smoke-all.list`
  gains it beside `--wallpaper` (`:162`); its line in CLAUDE.md's leg list.
- Verify: `just lint`, `just smoke --wallpaper-pair`, `just smoke
  --theme-toggle --wallpaper`, `just smoke --theme-auto`, `just smoke --lock`.

## Task 2: the pairing model

- `crates/fs-menu/src/providers/wallpaper.rs`: `Item` (`id`, `kind`:
  `Pair { light, dark }`, `Single { path, filed: Option<Mode> }`, `Sky`;
  `title`, `credit: Option<Credit>`), `items(paths, base_dir, identical)`
  per spec §3's four rules, `title_of(stem)`, `catalog(json, dir)` for
  the built-ins. The old variant functions stay until Task 3 deletes their
  callers.
- Unit tests: the owner's listing from spec §1 as names, with a predicate
  answering identical for the five twins, giving 1 pair, 4 both-mode, 28
  dark, 2 light and 19 unfiled; `fold-l.jxl`/`fold-d.jxl`; a `-dark` token
  in `Light/` named by the token; `-flexoki`/`-zenbones` left apart; two
  dark candidates for one light; titles for every stem in the owner's
  listing; a catalog entry with a missing file dropped.
- `crates/fs-menu/src/thumbnails.rs:7-25, 81-101`: `Mode::Wide`, a 640x360
  cover crop (`scale=640:360:force_original_aspect_ratio=increase,crop=640:360`),
  named `-wide640`; the test at `:107-117` covers it.
- `crates/fs-theme/src/tokens.rs`: `pickerTile` 180 in
  `SEMANTIC_SPACING_BASE`, reaching surfaces as `space.picker_tile`.
- `dev/smoke-map.toml`: `crates/fs-menu/src/providers/wallpaper.rs` gets a
  rule of its own running `--picker` and `--wheel` (it fell to the
  `providers/**` rule at `:209-210`, which runs `--menu` alone).
- Verify: `just lint` (fs-menu's tests are in it), `just cargo test -p fs-menu`.

## Task 3: the picker rebuilt

- `services/picker.rs`: `Scan` reads `picker.directory` (else
  `$XDG_PICTURES_DIR/Wallpapers` through the reader `services/menu.rs:446`
  uses) and `FS_WALLPAPERS`'s `catalog.json`; the byte-identity check on
  the pool for same-name files only (lengths, then bytes); publishes
  `Vec<Item>`; warms `Mode::Wide`; `decode` (`:137-154`) at the tile's
  width and height times the scale, a pair composed across the diagonal
  and every tile's corners masked at `lg` in device pixels, the Sky drawn
  as today.
- `surfaces/launcher.rs`: the picker an app view (`:760-777`); rows from
  items with `section` and kind; the layout (`:1331-1360`) laying sections
  under headings with `columns_for` and 16:9 cells plus the caption; the
  cell (`:3763-3777`) a `Kind::Picture` sized by width and height, its
  caption, the badge and Task 5's `checked` per spec §5; the cursor opening
  on the current tile; Ctrl+Tab and Ctrl+Shift+Tab by section with the
  "Next section" hint; Page Up and Down, Home and End; empty-state lines.
  `ui/el.rs` and `ui/draw.rs`'s `Kind::Picture` (width, height, badge) and
  `ui/w.rs:63-69`'s constructors.
- `choose_image` (`launcher.rs:937-953`) through `Choice`; select mode one
  flat section.
- Delete per spec §7: the variant model and switch, `KeyAction::Variant`
  (`launcher.rs:2558-2561`, `wayland/launcher.rs:1347-1350`),
  `actions::Variant` and `variant_switch` (`fs-menu/src/actions.rs:434-449,
  511-512, 622`), `LAUNCHER.picker_columns`, `picker_cell_px`, `bmp` in the
  scan.
- `fs-menu/src/actions.rs:283-284`: the image column per spec §5, run as
  `wallpaper.setFor:<mode>:<path>` in process.
- `ipc/picker.rs`: `variant` gone, `chooseFor` added, `status` per spec §6;
  `ipc/golden.rs:444-455` and `ipc-golden.jsonl:437-446` follow.
- `dev/smoke.d/picker.sh` rewritten per spec §8 (its fixture at `:66-96`
  grows the cases; a fixture catalog under the leg's own `FS_WALLPAPERS`;
  `PICKER_INCLUDED=real` leaves the result's own set in place, as
  `SMOKE_WALLPAPER_DITHER=1` flips `--wallpaper`). Tile rects off `menu
  status`'s `rowBoxes`; corner pixels read for the badge and the current
  mark; the stale audit kept. Grid, a pair under the cursor, the actions
  column and the empty state photographed, dark and `--light`.
- `dev/smoke.d/wheel.sh:62-103`: the scroll read over the new layout.
- `dev/smoke-map.toml`: the picker rule (`:595-596`) and the thumbnails
  rule (`:701-702`) run `--picker`, `--picker --light` and `--wheel`;
  `dev/smoke-all.list:125` gains `--picker --light`; CLAUDE.md's `--picker`
  line rewritten.
- Verify: `just lint`, `just smoke --picker`, `just smoke --picker
  --light`, `just smoke --wheel`, `just smoke --clipboard` (shares the
  launcher surface and waits on `picker_done_path`).

## Task 4: Style previews and the Settings door

- `launcher.rs`: the Style section's two `mode` tiles, each its slot's
  picture through `source`, the effective mode's tile ringed, the auto
  caption off `theme status`'s schedule, Enter running `theme mode
  light|dark` and showing its error under a pinned `theme.mode`; hidden
  while a query stands.
- `surfaces/panel/settings_page.rs:31-36`: the Appearance row becomes
  ("appearance", "image", "Wallpaper", "route:wallpaper") with the current
  title as its value; `Picker` records the level it was summoned from so
  `pop` (`launcher.rs:1855-1875`) returns there.
- `--picker`: the two Style tiles read off `picker status` (`kind`
  `mode`), Enter on Light flipping `theme status`'s mode, the previews'
  pictures matching the slots after a pair pick (crop means). Photographed
  dark and light.
- `dev/smoke.d/settings_ui.sh`: Appearance's last row opens the picker in
  the card (`menu status` `level` `wallpaper`), Backspace lands on
  Appearance. Coordinate with the agent editing that file now (it is
  modified in the main checkout): add lines, keep theirs.
- Verify: `just lint`, `just smoke --picker`, `just smoke --picker
  --light`, `just smoke --settings-ui`, `just smoke --settings-ui --light`.

## Task 5: the bundled set

- `dev/tarball.sh`: the gnome-backgrounds 51.0.1 tarball pinned beside the
  Blue Marble (`:90-96`) with spec §4's URL and sha256; `libjxl-tools` in
  the container's install list (`:24-30`), checked with `djxl --version`;
  `packaging/wallpapers.py` run in the art cache step (`:217-226`, its
  file in `art_key`); the release's `formalshell-depth` run over the staged
  directory; `share/formalshell/wallpapers/` copied in;
  `FS_WALLPAPERS` exported in the launcher (`:240-246`).
- `packaging/wallpapers.py`: per spec §4, the six pairs by file name, the
  catalog with each `source` URL and the embedded author read off the XMP
  and asserted against the table, `COPYING` copied. Curation: matugen's
  rank 0 read on each half (`matugen image --dry-run -d`, the probe
  `services/theme.rs` runs) and a pair over 60 degrees apart dropped with
  a line saying so.
- `dev/build.sh:36-75`: `FS_WALLPAPERS` exported; `wallpapers` linked from
  the host package's share when present, not required.
- Verify: `just tarball`; `tar tf` lists twelve JPEGs, twelve `.depth`
  maps, `catalog.json` and `COPYING`; each JPEG's ICC is sRGB (`identify
  -verbose`); `FS_RESULT=<unpacked tarball> PICKER_INCLUDED=real just smoke
  --picker`, the Included frame read for six pair tiles with captions and
  credits, dark and light.

## Task 6: docs and the hosts

- `docs/USAGE.md:3393-3445`: the Picker section per the spec (sections,
  pairs and how they are found, two slots, Style, built-ins and credits,
  the verbs); `:1026-1058` (flat colour and Sky) says a pick covers both
  slots; the `wallpaper` verbs at `:736-741`.
- `docs/DESIGN.md:966-967`: the Picker line (16:9 tiles at `lg`, the
  diagonal pair, the badge and the mark, Style previews).
- `docs/superpowers/specs/2026-10-31-settings-ui.md` §3: Appearance's last
  row is Wallpaper.
- Ship: bump the formalshell `rev` in `~/.config/formalspec/system.star`,
  `fspec update formalshell`, commit and push the spec (with Task 1's
  mirror change), `fspec apply --host g815` and `--host e1504g`. Read
  `formalshell-ipc call picker status` on g815 (the live shell, read-only:
  no surface driven) for the owner's directory giving spec §1's 54 tiles.
- Verify: `rg -n 'picker variant|Dark / Light|Dark \| Light' docs CLAUDE.md`
  returns nothing; `just smoke-affected` runs nothing.

## Task 7: GNOME side by side

- A throwaway GNOME VM on g815 (the `host-ops` skill's distro VM) with
  `gnome-shell --version` reading 51 and gnome-backgrounds 51.0.1
  installed; Settings, Appearance open.
- Capture GNOME at scale 1 and 1.25, dark and light (`gsettings set
  org.gnome.desktop.interface color-scheme`): the page at rest with Fold
  set. Ours: `FS_RESULT=<unpacked tarball> PICKER_INCLUDED=real just smoke
  --picker` and `--picker --light`, Fold set, scrolled to Included.
- Compose each pair at equal physical size with ImageMagick into
  `artifacts/wallpaper-picker-vs-gnome-{dark,light}.png` and read them:
  preview placement, tile size and gap, sections, the pair split, the
  current mark, and the colour of each converted half against GNOME's own
  draw of the same file (mean difference read at four points). A structural
  difference is fixed in this commit; a material one (radius, fill, ring)
  is listed in spec §8 as the table's.
- Remove the VM.
- Verify: both sheets read, `just smoke --picker` green after any fix.
