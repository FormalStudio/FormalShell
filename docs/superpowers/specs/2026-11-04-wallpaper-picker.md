# Wallpaper picker

Date: 2026-11-04. Owner's call: "the current one is bad, doesn't
distinguish our built in one and i want a better integration for choosing
light and dark wallpapers (some of them are the same image just light mode
for instance)". The look is the house one: Pond's composition and scale,
GNOME's colours, sans type and behaviour, FormalShell's own controls.

This spec wins over `docs/DESIGN.md`'s "Picker" line (`:966-967`), over
USAGE.md's Picker section (`:3393-3445`), and over the settings-ui spec
(`2026-10-31-settings-ui.md` §3) for the row Appearance ends on. The
panel-language spec still decides type, colour roles and empty states; the
theme-boundary spec still gives radii, fills and clocks to the table
(`crates/fs-theme/formalshell.json`), so no GNOME or Pond colour lands in
code.

## 1. What is there today

- The picker is the launcher's `wallpaper` route (`surfaces/launcher.rs:426-443`,
  `:882-1005`): one `find` over `picker.directory` and its `Dark`/`Light`
  subdirectories (`crates/fs-menu/src/providers/wallpaper.rs:32-45`), a
  4-column grid of square cover crops (`fs_theme::tokens::LAUNCHER.picker_columns`,
  `tokens.rs:343`), the Sky cell last (`launcher.rs:901-908`), and a
  Dark | Light segmented switch when either subdirectory exists
  (`launcher.rs:4201-4204`). The switch is a browsing filter: one set at a
  time, and a pick out of a set flips the theme to that set's mode
  (`wallpaper.rs:113-124`, commit 34215d23).
- Built-in: only the Sky. The release bundles no picture
  (`dev/tarball.sh:217-232` stages templates, branding, fonts, the Blue
  Marble globe and earbuds art; `branding/` holds `screensaver.txt` alone).
  Nothing in the grid says which cell is the shell's and which is the
  user's.
- state.json holds one picture (`services/state.rs:25-29`); every reader of
  the desktop takes `Store::desktop()` (`store.rs:139-143`): backdrops
  (`wayland/heads.rs:388`), the lock (`wayland/lock.rs:1146`), the band's
  paint, rims, the desktop clock, the overview, and the theme engine's
  inputs (`wayland.rs:2844-2847`).
- The current mark (pond-polish Task 5) is landing as this is written:
  `w::checked_picture` (`ui/w.rs:67-69`) and the disc drawn in
  `ui/draw.rs`'s `Kind::Picture` arm, used at `launcher.rs:3768`. This spec
  builds on it and does not redo it.

The owner's own directory (`~/Pictures/Wallpapers`, synced by Syncthing to
the hosts) shows what the model has to carry. Read 2026-11-04 on the mac:

| What | Files |
| --- | --- |
| Root | 19 pictures, hidden today because `dark/` and `light/` exist (USAGE.md: "Files sitting directly in the directory are not listed in that case") |
| `dark/` | 33 |
| `light/` | 7 |
| Same name in both, byte-identical | `clouds.61220450.png`, `field.5aa40e21.png`, `pylon.7a79ac39.png`, `water.2d2b7d25.png`: one picture filed for both modes |
| A real pair | `dark/flexoki-dark-orb.png` and `light/flexoki-light-orb.png`: the same 3840x2160 dithered orb, light on black and black on cream |
| Same name in root and `dark/`, identical | `flexoki-dark-orb.png` |
| Siblings that are not a pair | `wallhaven-yq2zwl-flexoki.png` and `-zenbones.png` (two pinned palettes) |

Under this spec that directory reads as 53 tiles: 1 pair, 4 both-mode
pictures, 28 dark-only, 2 light-only and 18 unfiled, with nothing hidden.

## 2. The references

GNOME 51.1 (`gnome-control-center`, tag `51.1`, `panels/background/`):

| Thing | GNOME | Source |
| --- | --- | --- |
| Page | Style (two preview toggles, Default and Dark), Accent Color, Background with "Add Picture…" in its header | `cc-background-panel.blp:26, 120, 137-151` |
| Preview cards | 4:3, 256 natural width, the background with two mock windows; Default paints the light picture, Dark the dark one; choosing one writes `color-scheme` | `cc-background-preview.c:26, 141-178, 246`, `cc-background-panel.c:229-246` |
| Grid | user pictures on top, a separator, then the built-ins; tiles 144x108, 12 apart, 1 to 8 a line, single click | `cc-background-chooser.blp:6-54`, `cc-background-chooser.c:34-35` |
| A pair in one tile | split down the middle, light left and dark right, swapped for RTL | `cc-background-paintable.c:254-284` |
| Setting | `picture-uri` and `picture-uri-dark` written together; a single picture writes the same uri to both; the lock gets the light one | `cc-background-panel.c:346-401` |
| Current mark | an accent disc with a check, bottom-right, fading in over 200ms | `shell/style.css:123-135` |
| Added pictures | copied into `$XDG_DATA_HOME/backgrounds/`, removable with an Undo toast | `bg-recent-source.c:334-389`, `cc-background-chooser.c:130-183` |
| Credits | none on screen; the name is the accessible label | `cc-background-chooser.c:249-250` |

gnome-backgrounds 51.0.1: 25 entries, every one a pair, described by
`<filename>` and `<filename-dark>` (`backgrounds/fold.xml.in:4-12`), files
named `<name>-l.jxl` and `<name>-d.jxl`, 4096x4096 JPEG XL, CC BY-SA 3.0 US
(`COPYING:1-4`), authors embedded in each file's XMP (`AUTHORS:1`).

Pond (`artifacts/pond/src`, `pond-2026-11-04`): a 624-wide modal, two
columns of 276x155 tiles 12 apart, radius 12
(`shell/settings/appearance/WallpaperPicker.qml:13-37`, `WallpaperTile.qml:16-28`);
the order is "Choose an image", the user's pictures, the Daylight sky, then
14 included pictures in one unlabelled grid (`wallpaper/control.py:127-130`);
title and author on a gradient on hover or focus (`WallpaperTile.qml:55-78`);
a white disc with a check, bottom-right (`:80-90`); a flat catalog of
`file`, `title`, `author` (`wallpaper/catalog.json:1-16`); thumbnails 800
wide, keyed on path, mtime and size (`control.py:64-96`); the desktop
decoded at the output's pixel size (`wallpaper/desktop/Content.qml:31`). No
light/dark anywhere, no licence per picture.

What we take: GNOME's two slots, its Style previews over the grid, its
sections, its single click; Pond's tile scale, 16:9 tiles, radius,
captions and catalog with credits. What we do not take: the vertical split
(§5), mock windows in the previews, Add Picture, Pond's hover-only captions.

## 3. Pairs

### Detection

The scan is the same `find` (`picker_scan_command`); what changes is how
its listing is read, in fs-menu as a pure function over paths and a
predicate saying whether two files are byte-identical. Files are taken in
sorted order so the result is deterministic.

1. **Same name across `Dark` and `Light`** (either case). Identical bytes:
   one picture filed for both modes (the owner's four). Different bytes: a
   pair, the directory naming each half. Identical to a copy in the root:
   one picture, filed as its subdirectory says.
2. **Names that differ by a mode token**, wherever the two files sit. The
   stem is cut into tokens on `-`, `_`, `.`, spaces and brackets; a whole
   token `dark` against `light` (any case), or a last token `d` against
   `l` (gnome-backgrounds' `fold-d` and `fold-l`), with every other token
   equal. The extension may differ. The token names each half, whatever
   directory the file is in. A token with no sibling is just a word.
3. **The built-in catalog** names its pairs outright (§4).
4. Everything left is a single: **filed** dark or light when it sits in
   that subdirectory, **unfiled** otherwise (the root, and the both-mode
   twins of rule 1).

Two dark candidates for one light half: the one in the same directory
wins, then the first by name; the other stays a single. No sidecar file
for user pairs: the owner already sorts by directory and by name, and a
file to maintain beside 50 pictures is the kind of upkeep he is asking to
lose. Reading `gnome-background-properties` XML from `/usr/share` is out:
it is GNOME's, and a Hyprland desk rarely has it.

### Titles

A tile's title is its catalog title, else its stem: the mode token, any
`<w>x<h>` token and any all-digit token of five or more digits dropped,
the rest joined with spaces, the first letter upper-case
(`flexoki-dark-orb` reads "Flexoki orb", `3840x2160_mlfw_watchtower-mountains-and-forests`
"Mlfw watchtower mountains and forests"). An empty result falls back to
the file name. Search matches the title and the file names.

### Two slots

state.json keeps `wallpaper` and gains `wallpaperDark`, GNOME's
`picture-uri` and `picture-uri-dark`: `wallpaper` is the light slot and
the picture for both modes while `wallpaperDark` is empty. `wallpaperColor`
(a flat colour or `sky`) still covers both slots, and `wallpaper.color` in
settings.json still sits under the picker's choice as it does today
(`services/wallpaper.rs:41-53`).

| Picked | Written |
| --- | --- |
| a pair | `wallpaper` the light half, `wallpaperDark` the dark half, `wallpaperColor` false; mode untouched |
| an unfiled single | `wallpaper` the picture, `wallpaperDark` empty, `wallpaperColor` false; mode untouched |
| a filed single | that slot only (filing the light slot while `wallpaperDark` is empty first copies the old `wallpaper` into `wallpaperDark`, so the dark slot keeps what it showed); the mode switches to the filing, as 34215d23 does today, unless settings.json pins `theme.mode`. Under `auto` the switch is the one-cycle snooze `theme mode` already writes |
| "Use in light mode only", "Use in dark mode only" on any single | that slot only, mode untouched |
| the Sky | `wallpaperColor` `sky`, slots kept under it |
| any slot pick while a colour or the sky is up | both slots, so a stale picture never comes back under the other mode |

The mode written goes first in the same write (`state::set_wallpaper`'s
rule, `state.rs:350-365`).

### What the desktop shows

`wallpaper::source` (`services/wallpaper.rs:41-53`) takes the mode and the
dark slot: in dark mode a non-empty `wallpaperDark` is the picture, else
`wallpaper`. That is the one answer every reader already takes, so:

- A mode change (the toggle, `theme mode`, the Style previews, `auto` at
  sunset) changes `Store::desktop()`; the backdrops ask for the other half
  (`heads.rs:383-405`, run on `Topic::State`, `wayland.rs:869`) and
  crossfade on `reveal` as any wallpaper change does. Nothing new draws
  the fade.
- The theme engine reruns matugen on the half shown: it already reruns on a
  change of wallpaper or mode (`services/theme.rs:532`). Each half seeds its
  own palette, rank 0 pinned as today. The header note at `theme.rs:31-33`
  is about `--prefer` per mode on one picture; a pair is two pictures and
  each half gets the colour it shows. A pinned palette in a half's path
  (the owner's `flexoki-*-orb`) pins it in both modes.
- The lock shows the half the mode shows, not GNOME's light-only lock: our
  lock is the session's own surface in the session's palette, and a light
  picture under a dark lock's ink would fight the `lock.ink` contrast read.
  A mode change while locked crossfades under the lock the same way.
- The greeter shows the mirrored copy. The mirror
  (`~/.config/formalspec/modules/linux/rootfs/usr/local/libexec/formalshell-greeter-mirror`)
  copies the half state.json's `mode` selects:
  `jq -r 'if .mode == "dark" and (.wallpaperDark // "") != "" then .wallpaperDark else .wallpaper // empty end'`.
- Depth maps stay per picture (`.depth/<name>.png` beside each half,
  `wallpaper.rs:112-120`), so each half carries its own subject cut.
- `wallpaper get` answers the picture shown now; `theme status` gains
  `wallpaperDark`.

## 4. Built-ins

### What ships

Nine nature photographs, every one CC0 1.0, each a single covering both
modes:

| id | Title | Photographer | Pixels | Palette |
| --- | --- | --- | --- | --- |
| `lake-matheson` | Lake Matheson | Jingwei Ke | 4811x3207 | violet dawn, mirror lake |
| `lake-placid` | Lake Placid | Alex Shutin | 4135x2760 | blue ridges in haze |
| `dunes` | Dunes | Andrzej Kryszpiniuk | 6000x4000 | orange sand, grey sky |
| `pine-forest` | Pine forest | Andrew Coelho | 7360x4912 | deep green, mist |
| `hazy-ridges` | Hazy ridges | Fré Sonneveld | 4608x3072 | teal ridges, gold sky |
| `shoreline` | Shoreline | David Marcu | 4288x2848 | teal sea, pale sand |
| `lofoten` | Lofoten | Johannes Groll | 5059x3373 | night, green aurora |
| `aletsch` | Aletsch | Sam Ferrara | 8196x5464 | slate glacier, pink sky |
| `winter-sunrise` | Winter sunrise | Jeremy Bishop | 5184x3456 | yellow sky, navy cloud sea |

Each is a Wikimedia Commons copy of a photograph Unsplash published before
5 June 2017, when Unsplash released under CC0, and Commons' licence review
confirmed it; the Commons original is the pinned file, so the licence is
CC0 rather than the Unsplash Licence and the URL is Commons' content path,
which does not change. None is a photograph elementary OS (every tag of
`elementary/wallpapers`), GNOME or Pond (`pond-2026-11-04`) ships, and
none is by a photographer elementary OS or Pond credits. The nine
seed nine different palettes, two of them dark enough for a dark desk
(`lofoten`, `lake-matheson`).

The pins (licence `CC0 1.0`,
`https://creativecommons.org/publicdomain/zero/1.0/`, for all nine; source
pages under `https://commons.wikimedia.org/wiki/File:`, originals under
`https://upload.wikimedia.org/wikipedia/commons/`):

| id | Commons file (source page and original) | Original page | sha256 of the original |
| --- | --- | --- | --- |
| `lake-matheson` | `6/61/Morning_Fog_at_Lake_Matheson_(Unsplash).jpg` | `unsplash.com/photos/E7w_Ti4BSSs` | `e541c879683717b2dde3c47e47efd1b91e5dfea78171fc9a5a1d4b08dcbb1943` |
| `lake-placid` | `7/77/Lake_Placid,_NY_(Unsplash).jpg` | `unsplash.com/photos/kKvQJ6rK6S4` | `770fe559d26db797ada127c5e50a6197d97b4a488739548e34d2d496badb2df0` |
| `dunes` | `6/6d/Dry_Desolation_(Unsplash).jpg` | `unsplash.com/photos/D1IS5s5O9xo` | `741d06eb230a1924e33f655734071e10d6bfe13089befd430523b4e0befa2453` |
| `pine-forest` | `5/5e/Lush_green_pine_forest_(Unsplash).jpg` | `unsplash.com/photos/aL7SA1ASVdQ` | `0bfe3a3ba2f8c06db54fa6e9295ef2fffa3517197f5ab23ef3f61e9b65ba04af` |
| `hazy-ridges` | `0/0c/Hazy_mountaintops_at_dusk_(Unsplash).jpg` | `unsplash.com/photos/njEtyA05EN4` | `7e47c497dcd2c064662638344a4a888cf3c299c935f3f2977830d940185243d6` |
| `shoreline` | `f/fb/Ocean_and_beach_shore_(Unsplash).jpg` | `unsplash.com/photos/58Mkqady7lE` | `5f7a125009b9624495ab1068b1ccfe2cc7f3dfcb8daeabaf81604d24e2a566b2` |
| `lofoten` | `8/86/Lofoten,_Norway_(Unsplash).jpg` | `unsplash.com/photos/upXoQv5GAr8` | `3bccf7c95c7e453f448da8f2d660737adc01eaad8a5f115767321537143136a5` |
| `aletsch` | `f/f0/Sunrise_Bettmerhorn_snowy_mountains_(Unsplash).jpg` | `unsplash.com/photos/uOi3lg8fGl4` | `8af62d34b654a71abb81853b1f73d330992eb5d8aafb65ac8e52855b550f57a2` |
| `winter-sunrise` | `c/c0/A_winter's_sunrise_(Unsplash).jpg` | `unsplash.com/photos/6GqDIGk7HlE` | `f3d52565557f58ff9d433598ed1f17c55a0889d4ae9eebcc9a8f97ef5f276939` |

The source page is the Commons file page of the name after the hash
directories; the original is that path under the originals root,
percent-encoded (`,` as `%2C`, `'` as `%27`, brackets as `%28` and `%29`).
All nine hashes were read off a download on 2026-11-04 and match Commons'
own sha1 for each file.

Plus the Sky, drawn by the shell. No pair ships. A pair needs the same
frame by the same photographer by day and at night, so the composition
lines up across the diagonal (§5), and no CC0 or public-domain nature
photograph was found with such a twin; the nearest, W.carter's CC0 Govik
harbour jetty by day and in last light, moves the camera between the two.
Darkening one photograph into a "dark" half is not a pair and is not done.
Pairs stay what users' own folders and a later catalog entry carry, and a
catalog pair whose halves seed palettes more than 60 degrees of hue apart
(matugen's rank 0 read on each) is dropped at curation, so a toggle never
swings the shell's colour family.

### How it ships

Per CLAUDE.md's "Everything ships": `packaging/wallpapers.json` holds the
nine rows of the two tables above (`id`, `title`, `author`, `license`,
`licenseUrl`, `source`, `origin`, `url`, `sha256`). `dev/tarball.sh` reads
each `url` and `sha256` out of it and fetches the originals into its
download cache beside the Blue Marble (`tarball.sh:90-96`), with the same
verify-or-fetch step. A new `packaging/wallpapers.py`, run in the same art
cache as `earth-tiles.py` (`tarball.sh:217-226`), converts each original to
sRGB through its embedded ICC profile with Pillow's `ImageCms`
(`python3-pil` is already installed; `pine-forest` carries Adobe RGB
(1998), the rest sRGB or none, read as sRGB), scales any whose long edge
passes 5120 down to 5120 (Lanczos), and writes `<id>.jpg` at quality 92
with no metadata but an sRGB profile. The shell's `image` crate decodes
png, jpeg and webp only (`crates/formalshell-rs/Cargo.toml:44`), which all
nine are. The release's `formalshell-depth` then writes each picture's
depth map into `.depth/` there, since the share directory is read-only at
run time. Into `share/formalshell/wallpapers/`:

- the nine JPEGs and their `.depth/` maps;
- `catalog.json`, one object per picture:
  `{"id", "title", "author", "license": "CC0-1.0", "licenseUrl", "source": "<commons file page>", "origin": "<unsplash page>", "file"}`;
  a pair (none ships) has `light` and `dark` in place of `file`;
- `CREDITS`, one line per picture: title, photographer, source page,
  licence name and licence URL. CC0 asks for none of it; the photographers
  get it anyway.

`FS_WALLPAPERS` points the shell at that directory, exported by the
launcher script (`tarball.sh:240-246`) and by `dev/build.sh` (`:69-75`)
like `FS_EARTH_TILES`; `dev/build.sh` links it from the host's installed
package like the other art (`:49-53`). The owner's hosts get it with the
next `formalshell` rev bump in `system.star`; install.sh needs no new name,
since nothing new runs at run time. A tree with no such directory (a
`cargo run`, a package from before this) shows the Sky alone under
Included, with no complaint.

### How it reads as ours

- Its own section, **Included**, after the user's.
- Each tile's caption carries the photographer after the title, muted:
  "Lake Matheson · Jingwei Ke". The Sky's reads "Sky · Follows the sun".
- The section heading's far end carries "CC0" once (one licence for the
  whole set, and the Sky has none to state), and `picker status` carries
  each item's author, licence and source.

## 5. The picker

### Where it lives

One picker, the launcher's `wallpaper` route, reached three ways:

- `Super+Ctrl+Space`, the root's Wallpaper row, `picker summon`, as today.
- Settings, Appearance: the last row, today "Theme and wallpaper" handing
  over to the `theme` route (`surfaces/panel/settings_page.rs:31-36`),
  becomes "Wallpaper" (`image` glyph) with the current title as its muted
  value, and summons the route in the same card. Backspace from the
  picker reached this way goes back to Appearance, as Bar and Desktop do.
  The `theme` route stays at the root: Appearance already carries the mode
  as its Style choice (`services/overrides.rs:62`), and Retheme is a
  debugging verb.
- No panel and no surface of its own. Settings pages already live in the
  launcher card (settings-ui §3), so the two doors land in one place.

The route becomes a launcher app view like `monitor` and `mirror`
(`launcher.rs:760-777`): `popupWidthMenuApp` (900) wide, cut to the output,
`app_height_share` tall, the field and its token as every route has them.

### Layout

Three sections down one scrolling body, each under the launcher's heading
(`heading_el`, `caption` 600 `muted`, panel-language §5):

1. **Style**: two preview tiles, Light and Dark, the grid's tile size and
   gap, centred over the grid as GNOME centres its two. Each is the picture that mode shows now (its slot
   through `source`), captioned "Light" or "Dark"; the mode in effect
   carries the `ring` role's outline at rest (GNOME's checked ring, our
   ring). Under `auto` the heading's far end reads "Auto, dark until
   07:42" off `theme status`'s schedule. Enter or a click runs `theme mode
   light|dark` (its own rules: refused with "Set in settings.json" under a
   pinned `theme.mode`, a snooze under `auto`).
2. **Your wallpapers**, the far end the directory (`~/` shortened).
3. **Included**, the far end "CC0".

Tiles, every section alike:

| Thing | Value | Against |
| --- | --- | --- |
| Shape | 16:9, cover-cropped | GNOME 4:3 (144x108), Pond 16:9 (276x155) |
| Columns | `appgrid::columns_for(content + gap, pickerTile + gap)`, 4 at 900 | GNOME up to 8, Pond 2 |
| `pickerTile` (new spacing token, the minimum) | 180; 210x118 at 900 | |
| Gap | `lg` (12) both ways | 12 in both |
| Radius | `lg` (12) | Pond 12, GNOME 9 |
| Caption | `rowGap` under the picture, `caption` 500, the author or folder part in `muted`, one line, elided | Pond 13px on hover |
| Rest | no outline; the picture is the tile | |
| Hover | the `accent` wash under the caption, the picture untouched | |
| Cursor | the table's `ring` round picture and caption, keyboard only | |

16:9 because the outputs are 16:9 and 16:10 and a cover crop of that shape
is what the desktop shows. Four columns keep each half of a pair over 100
pixels wide.

A **pair** is one tile: one picture, the light half's crop above the
diagonal from the bottom-left corner to the top-right, the dark half's
below it, both the same crop of their own file, so the composition lines
up across the cut. Not GNOME's vertical split: each half gets a whole
corner and a whole edge, and the tile reads as one picture in two lights
rather than two pictures side by side. The composite and the `lg` corners
are made on the pool at decode, so `ui/draw.rs` keeps blitting one bitmap.

A **filed single** carries a mode badge top-left: Lucide `moon` or `sun`
in a disc of `card` at 0.8 with `foreground` ink, the size and inset of
the current mark's disc. A both-mode or unfiled single and the Sky carry
none.

The **current mark** is pond-polish Task 5's `checked` disc, bottom-right.
A tile carries it while it holds the slot the mode shows: a pair while it
holds both, a single while it is that slot's picture, the Sky while it is
up. The Style previews show both slots, so the other mode's picture is one
glance up. `Kind::Picture` grows from a square `size` to a width and a
height, and gains the badge beside `checked`.

### Keyboard and pointer

- The cursor opens on the current tile, scrolled into view; with none, on
  the first tile of Your wallpapers.
- Arrows move in 2D across section boundaries (Up from the first row of a
  section lands in the same column of the last row above, Style's two
  tiles included). Page Down and Page Up move by a viewport, Home and End
  to the ends, Ctrl+Tab and Ctrl+Shift+Tab to the next and previous
  section's first tile, named "Next section" in the foot.
- Enter or a click sets (§3); the card closes, as today.
- Tab opens the tile's actions column: Set wallpaper, Use in light mode
  only and Use in dark mode only (singles), Open file and Copy path (the
  half shown now, for a pair). Style tiles and the Sky have no column.
- Typing filters every section by title and file name; Style hides while a
  query stands. Escape clears the query, then leaves the route.
- The wheel and touchpad scroll as every launcher body does (`--wheel`).

### Empty and select states

- `picker.directory` unset: Your wallpapers reads `$XDG_PICTURES_DIR/Wallpapers`
  (`user-dirs.dirs`, the reader `services/menu.rs:446` uses), Pond's
  default. A directory that does not exist or holds no picture: one
  `body` line in `muted` under the heading, "No pictures in
  ~/Pictures/Wallpapers", and a caption "Set picker.directory in
  settings.json" under it (panel-language §7).
- No `FS_WALLPAPERS` directory: Included holds the Sky alone.
- A query nothing matches: "No wallpapers match" under the field.
- `picker select <dir> <token>` stays the generic image selector: one flat
  section of the directory's files (root and both subdirectories), no
  pairing, no Style, no Included, no badges, the answer written to
  picker-selection.txt as today.

## 6. IPC

`ipc-golden.jsonl` and the stub targets in `ipc/golden.rs` follow every
change here.

- `wallpaper set <path>`: both slots (an unfiled single), or a colour or
  `sky` as today.
- `wallpaper pair <light> <dark>`: new, both slots in one write.
- `wallpaper setFor <mode> <path>`: new, one slot, `mode` light or dark,
  the mode left alone; anything else answers
  `error: mode must be light or dark`.
- `wallpaper get`: the picture shown now.
- `picker summon`, `select`, `close`: unchanged.
- `picker choose <path>`: any listed file (either half of a pair, a single)
  or a Sky source; does what Enter on its tile does. Unchanged error.
- `picker chooseFor <path> <mode>`: new, the actions column's "Use in …
  mode only".
- `picker variant`: removed with the switch (no compatibility shim, per the
  owner's standing rule).
- `picker status`: `{open, mode, directory, included, count, cursor,
  cursorId, cachedThumbnails, current: {light, dark, shown}, sections:
  [{id, title, count}], items: [{id, section, kind, title, author,
  license, source, light, dark, filed, current}]}`, `kind` one of `mode`,
  `pair`, `single`, `sky`; `variant`, `hasVariants`, `darkCount` and
  `lightCount` go.
- `theme status`: gains `wallpaperDark`.

## 7. What goes

- `WallpaperVariants`, `Variant`, `wallpaper_variants`, `wallpaper_listing`,
  `wallpaper_pick_mode` (`providers/wallpaper.rs:47-124`).
- `Picker.light`, `picker_variants`, `picker_variant`, `picker_switch`,
  `set_picker_variant`, `picker_switch_h` and its band
  (`launcher.rs:160-163, 886-896, 924-927, 956-967, 1255, 1332-1334,
  3730-3733, 4201-4204`), the Ctrl+Tab variant hint (`:3565-3568`) and
  `KeyAction::Variant` (`:2558-2561`, `wayland/launcher.rs:1347-1350`),
  fs-menu's `actions::Variant` and `variant_switch` (`actions.rs:434-449,
  511-512`).
- `LAUNCHER.picker_columns` (`tokens.rs:319, 343`), replaced by
  `pickerTile`; `picker_cell_px` (`launcher.rs:4206-4212`), which sized
  thumbnails off the 560 `popupWidthMenu` while the card is 640, so cells
  were upscaled.
- `bmp` from the scan (`providers/wallpaper.rs:37`): the `image` crate is
  built without it, so a bmp listed in the grid could never be set.

## 8. Proof

- `--picker`, rewritten: a fixture directory with root pictures, `Dark/`
  and `Light/`, a same-name pair with different bytes, an identical twin
  in both, a `-dark`/`-light` pair across directories, an `x-l`/`x-d`
  pair, a dark-only and a light-only single, and `-flexoki`/`-zenbones`
  siblings; `FS_WALLPAPERS` a fixture catalog of two generated pairs with
  credits. Read off `picker status`: sections, kinds, counts, titles,
  nothing hidden. The pair tile's two halves read off the frame on either
  side of its diagonal; the badge and the current mark in their corners;
  Enter on a pair writing both slots; a dark-only pick writing one slot
  and the mode; Use in light mode only writing one slot and no mode; the
  cursor opening on the current tile; Ctrl+Tab to Included; the Sky; the
  empty Your wallpapers state; select mode flat; the stale audit over the
  cursor's walk. Dark and `--light`.
- `--wallpaper-pair`, new: a pair set over IPC; `theme mode toggle` both
  ways with a burst through the fade (a frame part way between the two
  halves' mean, none outside them), `theme status`'s `desktop` naming the
  dark half then the light, theme.json's primary following the half; the
  lock showing the mode's half; `theme.mode: "auto"` resolving to the half
  its `effective` names.
- `--settings-ui`: Appearance's Wallpaper row opening the picker in the
  card, Backspace back to Appearance.
- `--wheel`: the grid's scroll over the new tiles.
- A side-by-side sheet against GNOME 51.1's Appearance page in a VM,
  dark and light, `artifacts/wallpaper-picker-vs-gnome-{dark,light}.png`,
  each side showing its own built-ins; every structural difference fixed
  or listed here as the table's.

## 9. Against GNOME 51.0

Read 2026-11-04 in a throwaway Fedora 45 Workstation live VM on g815
(gnome-shell 51.0, gnome-control-center 51.0, gnome-backgrounds 51.0.1),
Settings, Appearance maximised with the `adwaita` pair set, at scale 1 and
1.25, dark and light, beside `PICKER_INCLUDED=real` `--picker` and
`--light` at the same scales:
`artifacts/wallpaper-picker-vs-gnome-{dark,light}.png`, each row one scale
so a logical pixel is one size across it. The included sets differ by
design (§4), so only how the page holds them is compared.

The same: Style first with two previews, the mode in effect ringed, a pick
writing `color-scheme`; the grid after it, single click, user pictures
before the built-ins; a pair as one tile with both halves; the current
mark a check disc bottom-right; no picture's credit drawn over it.

Fixed here: the two Style previews sat left in the grid's first two
columns; GNOME centres them, and so do we now.

Ours by design, not a defect:

- Accent Color: GNOME's row of nine accents sits between Style and
  Background. Our accent is the wallpaper's own palette (matugen,
  theme-boundary spec), so there is nothing to pick.
- Add Picture… in the Background header: not taken (§2); Your wallpapers
  is the folder, its path at the heading's far end.
- Sections: GNOME runs one grid under "Background", user pictures first
  past a separator; ours names Your wallpapers and Included, with credits
  and "CC0" (§4).
- Tiles: GNOME's 144 by 108 (4:3), three a line at its 576 clamp, no
  caption; ours 16:9 at `pickerTile` or wider, four a line in the 900
  card, a caption under each (§5).
- The pair split: GNOME's vertical, ours the diagonal (§5).
- Style previews: GNOME's are larger than its tiles, draw two mock windows
  over the picture and are captioned "Default" and "Dark" centred under
  them; ours are tiles of the grid's size, the picture alone, captioned
  "Light" and "Dark" as every tile is.
- Material: GNOME's boxed groups behind each section, its 9 radius, accent
  ring and disc; ours flat sections on the launcher card, `lg` corners,
  the table's `ring` and the `checked` disc.

