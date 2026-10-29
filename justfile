default: build
# Everything that compiles or runs the shell goes to g815's slot users, or
# the mac rig's while g815 is off (dev/remote.sh, FS_RIG): the mac itself
# has no pipewire, PAM or Wayland to link.
#
# cargo build --release, and ./result laid out as the release tarball
# unpacks (dev/build.sh).
build:
    ./dev/remote.sh build
# clippy over the workspace, then the pure crates' tests (dev/lint.sh).
lint:
    ./dev/remote.sh lint
smoke *FLAGS:
    ./dev/remote.sh smoke {{FLAGS}}
# The runs dev/smoke-map.toml maps this change to (against origin/main, or
# the base given, plus uncommitted files), over the free slots off one
# build. The default check for a change.
smoke-affected *BASE:
    ./dev/remote.sh smoke-affected {{BASE}}
# Every run in dev/smoke-all.list (or the list given): milestones only.
smoke-all *LIST:
    ./dev/remote.sh smoke-all {{LIST}}
cargo *ARGS:
    ./dev/remote.sh cargo {{ARGS}}
# fs-tray, fs-notifd, fs-mpris on a private bus (dev/check.sh); the
# daemon-backed ones need the VM rig.
check NAME:
    ./dev/remote.sh check {{NAME}}
# dev/smoke.sh in this machine's own session, on a Linux host.
local-smoke *FLAGS:
    ./dev/smoke.sh {{FLAGS}}
# The release tarball for this machine's arch, built in debian:bookworm as
# release.yml builds it (dev/tarball.sh), into artifacts/tarball/. The
# volume keeps the toolchains and cargo caches between runs.
tarball:
    ${CONTAINER:-docker} run --rm -v "$PWD:/src" -v formalshell-tarball:/build \
      -e VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/formalshell-rs/Cargo.toml | head -1)+$(git rev-parse --short HEAD)" \
      debian:bookworm /src/dev/tarball.sh /src/artifacts/tarball
# docs/shots/, the README's and the website's images and clips.
shots:
    ./scripts/shots
