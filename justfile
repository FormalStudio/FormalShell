default: build
build:
    git add -A && nix build .#formalshell
# The rig: g815's slot users (dev/remote.sh), the mac VM when g815 is down
# or a leg is vm_only. FS_RIG=vm forces the VM.
smoke *FLAGS:
    ./dev/remote.sh smoke {{FLAGS}}
cargo *ARGS:
    ./dev/remote.sh cargo {{ARGS}}
check NAME:
    ./dev/remote.sh check {{NAME}}
rig-lint:
    ./dev/remote.sh lint
# dev/smoke.sh in this machine's own session, on a Linux host.
local-smoke *FLAGS:
    ./dev/smoke.sh {{FLAGS}}
lint:
    git add -A && nix flake check -L
# The release tarball for this machine's arch, built in debian:bookworm as
# release.yml builds it (dev/tarball.sh), into artifacts/tarball/. The
# volume keeps the toolchains and cargo caches between runs.
tarball:
    ${CONTAINER:-docker} run --rm -v "$PWD:/src" -v formalshell-tarball:/build \
      -e VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/formalshell-rs/Cargo.toml | head -1)+$(git rev-parse --short HEAD)" \
      debian:bookworm /src/dev/tarball.sh /src/artifacts/tarball

vm-up:
    ./dev/vm.sh start
vm-down:
    ./dev/vm.sh stop
vm-build:
    ./dev/vm.sh prebuild
vm-lint:
    ./dev/vm.sh sync
    ./dev/vm.sh run 'git add -A && nix flake check -L'
# cargo inside the VM against the synced tree, in the rust package's own
# build environment (the mac has no pipewire or wayland to link).
vm-cargo *ARGS:
    @./dev/vm.sh sync >&2
    ./dev/vm.sh run 'git add -A && cd crates && nix develop ..#formalshell -c cargo {{ARGS}}'
vm-smoke *FLAGS:
    ./dev/vm.sh smoke {{FLAGS}}
# docs/shots/, the README's and the website's images and clips.
shots:
    ./scripts/shots
# nix/testvm.nix's services.greetd needs a rebuilt VM (`vm-down && vm-up`)
# before this passes for the first time — separate from vm-smoke since
# greetd's default_session is a standing system service, not a fresh nested
# compositor this recipe spins up itself (see dev/smoke-greeter.sh's own
# header comment). Pulls artifacts/greeter/ back with a plain scp, mirroring
# dev/vm.sh's own ssh/key wiring rather than adding a second command to that
# script for one caller.
vm-greeter *FLAGS:
    ./dev/vm.sh sync
    ./dev/vm.sh run './dev/smoke-greeter.sh {{FLAGS}}'
    mkdir -p artifacts/greeter
    ./dev/vm.sh pull artifacts/greeter artifacts/greeter
