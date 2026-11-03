# Bluetooth player source

**Date:** 2026-11-03
**Status:** done, on `main`
**Spec:** `docs/superpowers/specs/2026-11-03-bluetooth-player.md` (wins on conflict)

## Task 1: fs-bluez

- `state::Player`, the events, `PLAYER_IFACE` in the narrow subscription,
  `Bluez::player_command`; a state test for a player's track in milliseconds.
- Verify: `just cargo test -p fs-bluez --lib`.

## Task 2: the media source

- `media::Remote`, published by `devices::bluetooth`; rows, active, transport,
  volume and stream ownership in `services/media.rs`; the panel's icon and
  timeline, the lyrics gate.
- Verify: `just cargo test -p formalshell-rs --bin formalshell-rs media`.

## Task 3: the smoke leg

- `dev/bluez-player-stub.py`, `dev/smoke.d/bluez_player.sh`, mapped in
  `dev/smoke-map.toml` and `dev/smoke-all.list`, documented in CLAUDE.md.
- Verify: `just smoke-affected`, read the PNGs.
