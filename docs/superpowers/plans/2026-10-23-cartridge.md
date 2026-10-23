# The cartridge theme

**Date:** 2026-10-23
**Status:** planned, on `feat/cartridge` (worktree `../FormalShell-wt/cartridge`)
**Spec:** `docs/superpowers/specs/2026-10-23-cartridge-theme.md`. The
spec wins on conflict; its C-numbers are cited below.

One subagent per task, sequential, each ending in its verification run
read back and one commit (`feat(theme): ...` and the like). VM work goes
through `dev/vm-lock.sh just vm-smoke <flags>` from the worktree;
screenshots land in `artifacts/` and are read, never assumed. The site's
reference frames are in `artifacts/ref/`. Every task leaves
metamorphosis, retro and pantheon drawing exactly as before: a
`--gallery` frame under each is compared against `main`'s at the end of
any task that touches a shared widget.

## Task 1: type and curves (C2, C3)

- `fs-theme`: the table's `fonts` block (`words`, `values`, `display`,
  `body`) resolving down to the generic aliases; `pair`/`mono` unchanged
  for the other tables; `theme.fonts` in settings still wins.
- `text.rs`: `TextStyle` gains `wdth` and `wght` axes in the cache key,
  set through parley 0.11's style properties (read its source at the
  locked version); named families through `Family::Named`, falling back
  with one log line.
- `tokens`/`style.rs`: a curve may be a `[[t, v], ...]` stop list,
  evaluated piecewise linear like CSS `linear()`; `motion.rs` consumes
  it.
- `nix/cartridge-fonts.nix`: Archivo, Archivo Narrow, BIZ UD Mincho,
  DotGothic16 (OFL, pinned), on `FS_RS_FONT_DIRS` in `nix/package.nix`
  and in `dev/tarball.sh` with sha256.
- Check: `rust-tests`, with tests shaping Archivo at wdth 100 and 125
  (advance grows) and sampling a stop-list curve.

## Task 2: the table, habits and roles (C1, C4 statics)

- `themes/cartridge.json` overriding metamorphosis roles; `presets.rs`,
  `tables.rs`, the per-table test list in `style.rs`.
- New habits `emerge: slide`, `panel`, `grid`, `boot` and roles
  `cursor.bar`, `cursor.brackets`, `frame.chamfer`, `hero.tab`,
  `button.default.cap`, declared in all four tables, the other three
  taking today's look. The `ui::draw` box path learns chamfers, the bar
  and the brackets.
- `dev/smoke.d/cartridge.sh` rider; `docs/DESIGN.md`, `docs/USAGE.md`,
  `README.md` ("one of four looks"), CLAUDE.md's leg list.
- Check: `rust-tests`; `--cartridge --gallery`, `--cartridge --menu`
  frames read; `--gallery` under the other three unchanged.

## Task 3: the panel console grammar (C4 `panel`, C9 panels)

- `Panel::subtitle`, `status`, `accent` on every panel in
  `surfaces/panel/`; the `console` header in `host.rs`: title, `//
  subtitle` with glow, back chip, status strip, vertical id strip;
  section labels as field tags; the hero framed with its tab.
- Check: `--cartridge --panel <name>` for network, audio, media,
  bluetooth, calendar, power, monitor, weather; every frame read.

## Task 4: the motion catalogue (C5)

- `focusText`, `reveal: type`, row stagger, `swap: glitch`, `use`,
  `error: shake`, `picture: zoom`, `noise`, `progress: blocks`, each
  read off the table, absent everywhere else.
- Check: frame bursts under `debug motionScale` at a tenth speed for
  each effect (`--cartridge --menu`, `--cartridge --panel media`,
  `--cartridge --lock` for the shake, `--cartridge --osd`), read;
  `--cartridge --idle` shows no commits at rest from any of them.

## Task 5: the surfaces (C9)

- Launcher letterform, hex grids with brackets, notifications as the
  updates card, OSD blocks, lock slam-in and shake, switcher brackets,
  the bar's leading bar and typed clock.
- Check: `--cartridge` riding `--menu`, `--app-grid`, `--emoji`,
  `--picker`, `--notify`, `--osd`, `--lock`, `--switcher`, read.

## Task 6: the grid (C8)

- `theme.grid`, `wallpaper.grid`, `lock.grid` beside dither; the pass
  in `services/wallpaper.rs`, the lock and the greeter.
- Check: a unit test on a flat buffer; `--cartridge --wallpaper`,
  `--cartridge --lock` read.

## Task 7: the animated backdrop (C7)

- The shard sculpture as a backdrop layer, half resolution, at most
  24fps, every pause rule in C7, the last frame held.
- A leg `--backdrop` riding `--cartridge`: frames differ while visible,
  zero backdrop commits under a fullscreen window and with tiled
  windows covering the output, resumes after; budget read under
  `--idle` and under `FS_CPU_QUOTA`.
- Check: the leg, `--cartridge --idle`, frames read.

## Task 8: boot (C6)

- The greeter sequence, the once-per-login session boot (boot id in
  `state.json`, any key skips), the unlock short form.
- A leg `--boot` riding `--cartridge`: frame burst through the
  sequence, a key skipping it, no replay on a shell restart in the same
  boot; `just vm-greeter` under the cartridge preset.
- Check: the leg and `vm-greeter`, frames read.

## Task 9: sound (C10)

- `dev/sfx/gen.py` synthesising the nine sounds deterministically into
  `crates/fs-audio/sfx/`, committed with its output; the in-process
  PipeWire player, the event hooks, rate limits, DND and lock muting.
- A leg `--sfx` riding `--cartridge`: a null sink, streams counted on
  real Down, Return, Escape and typing in the launcher, none under DND,
  none without the rider.
- Check: the leg.

## Task 10: the whole look

- Every leg above together with `--showcase` on a flat teal wallpaper,
  read against `artifacts/ref/`; fix what reads wrong; record a
  launcher and a panel gif into `docs/media/cartridge-*.gif`; README
  section; deviations recorded at the top of this plan.
