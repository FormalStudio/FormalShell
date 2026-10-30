# Wallpaper depth

**Date:** 2026-10-26
**Status:** planned, on `main`
**Reference:** noctalia's `wallpaper_depth` plugin
(github.com/noctalia-dev/official-plugins, `wallpaper_depth/`, MIT): a
Depth Anything V2 Small mask per wallpaper so widgets sit behind the
scenery near the camera. FormalShell has no desktop widgets, so the one
surface that takes the mask is the lock: its clock passes behind the
wallpaper's subject, the way a phone's lock screen does.

## Shape

- **Maps, not masks, on disk.** One 8-bit grayscale PNG per wallpaper,
  the relative depth normalised to 0..255 (255 nearest), refined against
  the picture so its edges follow the subject's, at the picture's own
  aspect with its long edge capped at 2560. It lives beside the picture:
  `<dir>/.depth/<file name>.png`. The threshold and feather are applied by
  the shell at load, so changing them never needs a regeneration.
- **Generated on one machine, read everywhere.** `formalshell-depth` is a
  child CLI (Python, onnxruntime, numpy, pillow, the model pinned by hash in
  the flake). `wallpaper.depthGenerate` (default false) has the shell run
  it over `picker.directory` at start and whenever that directory changes;
  it writes a map only where one is missing or older than its picture. The
  wallpaper folder is already synced between the hosts (Syncthing, with
  `.depth/` riding along, and copies keep the sender's mtime), so g815
  generates and e1504g only reads.
- **Reading is on by default.** `lock.depth` (default true) uses a map
  when one exists; with none the lock draws exactly as before.
  `lock.depthThreshold` (0..100, default to be tuned in Task 2 against real
  wallpapers) and `lock.depthFeather` (0..50) shape the alpha:
  a smoothstep across `threshold +- feather/2` of the map's 0..100.
- **Composition.** The lock already bakes the scrim into its backdrop on
  the pool (`surfaces/lock.rs` `backdrop`). The foreground is the same
  pixels with the mask as alpha, built on the pool beside it. It is painted
  after the clock and before the body, cropped to the clock's own rect, so
  the subject passes in front of the digits and nothing else (the card's
  fill, the field, the date and now playing stay on top).

## Out of scope

- (Superseded 2026-10-30: the tarball carries the generator on a bundled
  CPython, see `dev/tarball.sh`; install.sh needs nothing for it.)
- The greeter keeps its wallpaper as it is.

One subagent per task, sequential, each ending in its verification run read
back and one commit.

## Task 1: the generator

- `nix/formalshell-depth.py` and `nix/formalshell-depth.nix`: Depth Anything
  V2 Small from `onnx-community/depth-anything-v2-small` at revision
  `4472b7362082ad9968fee890ca0f1e5aca36b93d`, `onnx/model.onnx`, sha256
  (the LFS oid, pinned in `nix/formalshell-depth.nix`)
  (fetchurl, never downloaded at runtime). Input at the picture's aspect,
  long edge 518, both sides multiples of 14, ImageNet mean and std
  (`preprocessor_config.json` at that revision is ground truth). Min-max
  normalise, upscale, refine with a guided filter on the picture's luma,
  write atomically (temp file, rename) with the map's mtime set to its
  picture's.
- `formalshell-depth <dir>` walks the directory and its subdirectories
  (never `.depth/` itself or other dot directories), skips up-to-date maps,
  prunes maps whose picture is gone, prints one line per picture.
  `formalshell-depth --file <picture>` does one.
- On the wrapper PATH in `nix/package.nix`; a `formalshell-depth` package
  output.
- Verify: `nix build .#packages.aarch64-linux.formalshell-depth`, then the
  CLI run in the VM over a directory holding one real photograph, and the
  map read by eye.

## Task 2: the shell

- Settings keys above in the settings schema and its docs, read the way
  sibling keys are.
- `services/wallpaper.rs`: the map's path for a source, its load and
  cover-crop to an output size (the same crop `image` makes), and the alpha
  from threshold and feather.
- `wayland/lock.rs` and `surfaces/lock.rs`: the foreground built on the pool
  beside the backdrop, painted after the clock cropped to its rect, with the
  crop cached while the rect holds still. `lock status` reports `depth`
  (whether a map is in use) per output.
- A depth service that runs `formalshell-depth` as a child on the service
  thread when `wallpaper.depthGenerate` is true, at start and on changes to
  `picker.directory`, one run at a time, and tells the lock to reload its
  foreground when a map for the current wallpaper lands.
- Unit tests for the path, the alpha curve and the crop.
- Verify: `just vm-cargo test -p formalshell-rs`, `just build`.

## Task 3: the leg

- `dev/smoke.d/lock_depth.sh` `--lock-depth`: a staged wallpaper and its
  map (a fixture: a bright disc over a dark field, the map white over the
  disc), the lock photographed, the clock's ink present where the map is
  dark and the wallpaper's own pixels where it is white, `lock status`
  reporting `depth` true; then `lock.depth: false` written under the
  running shell and the clock whole again. Then `formalshell-depth` run on
  the rig over a real photograph, the map it wrote read by eye.
- Its entry in `CLAUDE.md`'s leg list.
- Verify: `dev/vm-lock.sh just vm-smoke --lock-depth`, frames read.

## Task 4: the hosts

- g815: `programs.formalshell.settings.wallpaper.depthGenerate = true` in
  `systems/g815/default.nix`, the flake input bumped, rebuilt, and the
  maps checked in `~/Pictures/Wallpapers/.depth/`.
- e1504g: FormalShell's `rev` bumped in `system.star`, applied, and the
  maps arriving over Syncthing checked there.
