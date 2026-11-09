# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --config-reload proves a settings.json whose SYMLINK is retargeted still
# reaches a running shell.
#
# That is what home-manager does on every activation, and it is the one write
# a file watch cannot see: inotify resolves the path when the watch is added,
# so the watch sits on the store file, which is immutable and never touched
# again. Swapping the symlink beside it produces no event at all, and the
# shell stopped seeing its own config for the rest of the session (g815,
# 2026-08-26: a rebuild moved bar.position and the bar stayed where it was
# until the service was restarted by hand).
#
# A plain rename over the path is NOT the same case and does not test this:
# it unlinks the inode the watch is on, which inotify does report, so that
# path has always worked. The drive below therefore builds the symlink shape
# first and waits for the shell to be reading through it, and only then
# retargets it, which is the write that goes unannounced.
#
# The bar is the readout because its edge is a whole-screen fact this rig can
# read off the compositor: the fixture starts it on the right, the retarget
# asks for left, and the assert holds the layer box before and after. A shell
# that never re-read the file leaves the strip on the right and this fails.
# This leg owns the `bar` key, so it does not combine with --bar-position,
# --chevron or --bar-layout, which write the same key.
leg_config_reload_flag="--config-reload"
leg_config_reload_order=186
leg_config_reload_needs="jq"
leg_config_reload_claims="bar screen=shared"

config_reload_before_path="$shot_dir/config-reload-before.txt"
config_reload_monitors_path="$shot_dir/config-reload-monitors.json"
# The two "generations", named the way home-manager's own store paths differ
# from each other: the symlink points at one, then the other.
config_reload_after_path="$shot_dir/config-reload-after.txt"
config_reload_elapsed_path="$shot_dir/config-reload-elapsed.txt"
config_reload_shot_path="$shot_dir/config-reload.png"

leg_config_reload_fixture() {
  settings_pin '.bar.position = "right"'
}

leg_config_reload_timing() {
  # Config lands a few seconds in, the rewrite follows, and the shell has up
  # to 15s to notice it. The run's own frame is taken past all of that, so it
  # shows the strip on the edge the rewrite asked for.
  leg_timing 26 55
}

leg_config_reload_drive() {
  local script="$shot_dir/config-reload-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
# One box for the formalshell:bar layer, or empty while it has not mapped.
bar_box() {
  "$hyprctl_bin" -j layers 2>/dev/null \\
    | "$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first | "\\(.x) \\(.y) \\(.w) \\(.h)"' 2>/dev/null \\
    | grep -v null
}

wait_for_edge() {
  for _ in \$(seq 1 \$2); do
    box=\$(bar_box)
    case "\$box" in
      "") ;;
      *)
        x=\${box%% *}
        case "\$1" in
          right) [ "\$x" -gt 0 ] && return 0 ;;
          left) [ "\$x" -eq 0 ] && return 0 ;;
        esac
        ;;
    esac
    sleep 0.5
  done
  return 1
}

# The shape home-manager leaves behind: a symlink where the shell looks, onto
# a generation off to one side. Replacing the regular file with a symlink
# unlinks the inode the first watch was on, which inotify does report, so the
# shell re-reads and lands on that generation with its watch resolved onto
# THAT inode. Only then is the retarget a fair test.
settings_retarget .

wait_for_edge right 40
bar_box > "$config_reload_before_path"
"$hyprctl_bin" -j monitors > "$config_reload_monitors_path" 2>&1

# The retarget, the write nothing announces: the first generation is left
# exactly as it is, so the watch on it stays quiet forever and only a re-open
# finds the second.
settings_retarget '.bar.position = "left"'

# Up to 15s to notice, recorded so a regression that merely slows the pickup
# down is visible rather than silent.
start=\$SECONDS
wait_for_edge left 30
echo \$((SECONDS - start)) > "$config_reload_elapsed_path"
bar_box > "$config_reload_after_path"
"$grim_bin" "$config_reload_shot_path" > /dev/null 2>&1
EOS
  hypr_exec_once "bash $script"
}

leg_config_reload_assert() {
  local before after bx bw mw
  before=$(cat "$config_reload_before_path" 2>/dev/null)
  after=$(cat "$config_reload_after_path" 2>/dev/null)
  [ -n "$before" ] || fail "the bar never mapped on the fixture's own edge, so nothing was proven about re-reading"
  [ -n "$after" ] || fail "the bar layer went missing after settings.json was replaced"

  # The output's own logical width, dumped by the drive while the session was
  # still up: the assert runs after teardown, so hyprctl is gone by now.
  mw=$("$jq_bin" -r 'first | (.width / .scale | floor)' "$config_reload_monitors_path" 2>/dev/null)
  [ -n "$mw" ] && [ "$mw" != "null" ] || fail "no monitor dump produced for the config reload check"

  read -r bx _ bw _ <<< "$before"
  [ $((bx + bw)) -eq "$mw" ] || fail "the fixture asked for a right bar and got x=$bx w=$bw on a ${mw}-wide output"

  read -r bx _ bw _ <<< "$after"
  [ "$bx" -eq 0 ] || fail "the settings.json symlink was retargeted at bar.position left and the strip stayed at x=$bx: the shell never re-read the file"

  echo "config reload: bar moved right to left in $(cat "$config_reload_elapsed_path" 2>/dev/null)s after the settings.json symlink was retargeted"
  echo "SMOKE_CONFIG_RELOAD $config_reload_shot_path"
}
