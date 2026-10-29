#!/usr/bin/env bash
# The mac rig: fstestmac, an openSUSE Tumbleweed aarch64 guest on the
# macbook's Virtualization framework through lima. The owner's fspec spec
# declares both halves: hosts/fstestmac/lima.yaml is the VM (8 vCPU, 16 GiB,
# its own disk, nothing of the mac mounted) and hosts/fstestmac/host.star
# what runs in it (the slot users, a vkms card each, Hyprland and the legs'
# tools, the formalshell package built for aarch64). fspec's own vm() drives
# qemu on a KVM host only, hence this script.
#
# dev/remote.sh starts it when it falls back to the mac rig; this is the
# same by hand.
#
#   dev/mac-vm.sh start    create it from lima.yaml the first time, boot it,
#                          wait until a slot user answers ssh
#   dev/mac-vm.sh stop
#   dev/mac-vm.sh status   Running, Stopped or Missing
#   dev/mac-vm.sh apply    fspec apply --host fstestmac off the spec's HEAD,
#                          rebooting when the apply swapped the kernel
set -euo pipefail
name=fstestmac
spec="$HOME/.config/formalspec"

command -v limactl >/dev/null || { echo "mac-vm: limactl is not installed; it is in the macbook's fspec spec" >&2; exit 1; }

status() {
  limactl list --format '{{.Status}}' "$name" 2>/dev/null || echo Missing
}

ssh_ready() {
  ssh -o BatchMode=yes -o ConnectTimeout=5 "${1:-fstest0}@$name" true 2>/dev/null
}

start() {
  if [ "$(status)" = Missing ]; then
    limactl create --name "$name" --tty=false "$spec/hosts/fstestmac/lima.yaml"
  fi
  if [ "$(status)" != Running ]; then
    limactl start --tty=false "$name" > /dev/null
  fi
  for _ in $(seq 60); do
    ssh_ready && return 0
    ssh_ready kyan && break
    sleep 1
  done
  ssh_ready || { echo "mac-vm: $name is up but has no slot users yet: run dev/mac-vm.sh apply" >&2; return 1; }
}

case "${1:-}" in
  start)
    # Two agents falling back at once start it once.
    exec 9> /tmp/formalshell-mac-vm.lock
    python3 -c 'import fcntl, sys; fcntl.flock(int(sys.argv[1]), fcntl.LOCK_EX)' 9
    start
    ;;
  stop) limactl stop "$name" ;;
  status) status ;;
  apply)
    start 2>/dev/null || true
    (cd "$spec" && fspec apply --host "$name" --rev HEAD)
    # A first apply leaves sshd on its old config, which reads no
    # authorized_keys.d, and may leave the boot on kernel-default-base, which
    # has no vkms.
    if ! ssh_ready || ! ssh "kyan@$name" modinfo vkms >/dev/null 2>&1; then
      limactl restart --tty=false "$name" > /dev/null
      start
    fi
    ;;
  *) echo "usage: $0 {start|stop|status|apply}" >&2; exit 1 ;;
esac
