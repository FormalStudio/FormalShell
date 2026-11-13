# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --zen: `zen enter` under `layout: sidebar` and the scrolling layout, three
# foot windows a b c in one column each on workspace 1, columns 0.4 wide
# and b's widened to 0.6, b focused, with the owner's tape binds (Hyprland's
# layout `focus` and `movefocus`, formalwm's focus-column and focus-window).
# On: b floating on workspace 1, centred on the output within 2px, the
# sidebar folded to its rail, and a and c still on workspace 1 but wholly
# off the output (their tape of 0.8 is narrower than the output, the case
# Hyprland centres on every recalculate). Under formalwm (wm_zen=layout) b
# stays tiled instead, a goes off the left edge and c off the right, and the
# columns keep the settings' width, there being no socket call for one.
# Zen follows the focus: SUPER+L takes it to c and SUPER+H back to b, the
# switcher's focus (a dispatch) to a and back to b, each time the window
# before back in the tape off the output and the new one centred. Off: b
# tiled again between a and c (by x), at the width it had within 2px,
# focused and on the output, the sidebar open. Then c taken into b's column
# and zen on c: SUPER+K takes it to b and SUPER+J back to c, and off, c
# is under b in their column again. Then zen on b
# and b closed under it: zen stays on, on the window that took the focus,
# centred, the sidebar still folded; off once more, that window is on the
# output and the sidebar open. Then d and e opened after c, zen on c, and
# its neighbours changing under it while it keeps the focus: d closed,
# SUPER+L goes to e; c moved to workspace 2, SUPER+H goes to a; the config
# reloaded, SUPER+L goes to e again. The on, right and closed frames are
# pulled.
leg_zen_flag="--zen"
leg_zen_order=239
leg_zen_compositors="hyprland shoji"
leg_zen_needs="jq foot wtype"
leg_zen_claims="layout=sidebar frame=none"

zen_steps="before on right left switcher off stack up down restacked closed after four past-closed past-moved past-reload end"
zen_monitors="$shot_dir/zen-monitors.json"
zen_on_png="$shot_dir/zen-on.png"
zen_right_png="$shot_dir/zen-right.png"
zen_off_png="$shot_dir/zen-off.png"
zen_closed_png="$shot_dir/zen-closed.png"

leg_zen_fixture() {
  settings_pin '.layout = "sidebar"'
}

leg_zen_timing() {
  leg_timing 140
}

leg_zen_drive() {
  local script="$shot_dir/zen-drive.sh"
  need_wtype
  # The owner's binds: on Hyprland the layout's own column walk and
  # movefocus for a stack, after the include's block that records them.
  if [ "$wm_zen" = float ]; then
    wm_mac_apps
    wm_binds_by_keysym
    echo 'hl.bind("SUPER + H", hl.dsp.layout("focus l"))'
    echo 'hl.bind("SUPER + L", hl.dsp.layout("focus r"))'
    echo 'hl.bind("SUPER + J", hl.dsp.focus({ direction = "d" }))'
    echo 'hl.bind("SUPER + K", hl.dsp.focus({ direction = "u" }))'
    echo 'hl.bind("SUPER + comma", hl.dsp.layout("consume_or_expel prev"))'
    # In the config as well as set live, so the reload keeps the tape.
    echo 'hl.config({ general = { layout = "scrolling" }, scrolling = { column_width = 0.4 } })'
  else
    wm_bind SUPER+H focus-column left
    wm_bind SUPER+L focus-column right
    wm_bind SUPER+J focus-window down
    wm_bind SUPER+K focus-window up
    wm_bind SUPER+comma consume-or-expel prev
  fi
  write_script "$script" <<EOS
#!/usr/bin/env bash
record() {
  sleep 1.6
  $ipc call zen status > "$shot_dir/zen-\$1-status.json" 2>&1
  $ipc call sidebar status > "$shot_dir/zen-\$1-sidebar.json" 2>&1
  "$wmctl_bin" -j clients > "$shot_dir/zen-\$1-clients.json" 2>&1
  "$wmctl_bin" -j activewindow > "$shot_dir/zen-\$1-active.json" 2>&1
}
super() { "$wtype_bin" -M logo -k "\$1" -m logo; }
focus() { "$wmctl_bin" dispatch "hl.dsp.focus({ window = \"class:formalshell-zen-\$1\" })" > /dev/null 2>&1; }
for _ in \$(seq 1 60); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.25
done
"$wmctl_bin" eval 'hl.config({ general = { layout = "scrolling" }, scrolling = { column_width = 0.4 } })' > /dev/null 2>&1
spawn() {
  "$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-zen-\$1 sh -c 'sleep 300']==])" > /dev/null 2>&1
  for _ in \$(seq 1 40); do
    "$wmctl_bin" -j clients | "$jq_bin" -e --arg c "formalshell-zen-\$1" 'any(.[]; .class == \$c)' > /dev/null 2>&1 && break
    sleep 0.1
  done
  sleep 0.4
}
for n in a b c; do spawn \$n; done
focus b
"$wmctl_bin" dispatch 'hl.dsp.layout("colresize 0.6")' > /dev/null 2>&1
sleep 1
"$wmctl_bin" -j monitors > "$zen_monitors" 2>&1
record before
$ipc call zen enter > /dev/null 2>&1
record on
"$grim_bin" "$zen_on_png" > /dev/null 2>&1
super l; record right
"$grim_bin" "$zen_right_png" > /dev/null 2>&1
super h; record left
focus a; record switcher
focus b; sleep 1.6
$ipc call zen exit > /dev/null 2>&1
record off
"$grim_bin" "$zen_off_png" > /dev/null 2>&1
focus c; sleep 1
super comma; sleep 1
$ipc call zen enter > /dev/null 2>&1
record stack
super k; record up
super j; record down
$ipc call zen exit > /dev/null 2>&1
record restacked
focus b; sleep 1
$ipc call zen enter > /dev/null 2>&1
sleep 1.6
"$wmctl_bin" dispatch 'hl.dsp.window.close({ window = "class:formalshell-zen-b" })' > /dev/null 2>&1
record closed
"$grim_bin" "$zen_closed_png" > /dev/null 2>&1
$ipc call zen exit > /dev/null 2>&1
record after
focus c; sleep 0.6; spawn d; focus d; sleep 0.6; spawn e
focus c; sleep 1
$ipc call zen enter > /dev/null 2>&1
record four
"$wmctl_bin" dispatch 'hl.dsp.window.close({ window = "class:formalshell-zen-d" })' > /dev/null 2>&1
sleep 1.6
super l; record past-closed
"$wmctl_bin" dispatch 'hl.dsp.window.move({ window = "class:formalshell-zen-c", workspace = 2, follow = false })' > /dev/null 2>&1
sleep 1.6
super h; record past-moved
"$wmctl_bin" reload > /dev/null 2>&1
sleep 2
super l; record past-reload
$ipc call zen exit > /dev/null 2>&1
record end
EOS
  wm_exec_once "bash $script"
}

zen_file() {
  echo "$shot_dir/zen-$1-$2.json"
}

zen_client() {
  "$jq_bin" -c --arg c "formalshell-zen-$2" '.[] | select(.class == $c)' "$(zen_file "$1" clients)"
}

zen_on_output() {
  "$jq_bin" -e -n --argjson w "$1" --argjson m "$2" '$w.at[0] >= $m.x and $w.at[0] + $w.size[0] <= $m.x + $m.w' > /dev/null
}

zen_off_output() {
  "$jq_bin" -e -n --argjson w "$1" --argjson m "$2" \
    '$w.at[0] + $w.size[0] <= $m.x or $w.at[0] >= $m.x + $m.w or $w.at[1] + $w.size[1] <= $m.y or $w.at[1] >= $m.y + $m.h' > /dev/null
}

# zen_holds <step> <window> <mon> <others...>: zen on, on that window, which
# has the focus and sits centred, the sidebar folded, every other window
# named on workspace 1 and off the output, tiled again under Hyprland.
zen_holds() {
  local step=$1 n=$2 mon=$3 w cx f o
  shift 3
  "$jq_bin" -e '.on == true' "$(zen_file "$step" status)" > /dev/null || fail "zen $step: not on: $(cat "$(zen_file "$step" status)")"
  "$jq_bin" -e '.collapsed == true' "$(zen_file "$step" sidebar)" > /dev/null || fail "zen $step: the sidebar is not on its rail"
  "$jq_bin" -e --arg c "formalshell-zen-$n" '.class == $c' "$(zen_file "$step" active)" > /dev/null \
    || fail "zen $step: the focus is on $("$jq_bin" -r '.class' "$(zen_file "$step" active)"), not $n"
  w=$(zen_client "$step" "$n")
  echo "zen $step $n: $("$jq_bin" -c '{at, size, floating, ws: .workspace.id}' <<< "$w")"
  "$jq_bin" -e --argjson fl "$([ "$wm_zen" = float ] && echo true || echo false)" '.floating == $fl and .workspace.id == 1' <<< "$w" > /dev/null \
    || fail "zen $step: $n is not $([ "$wm_zen" = float ] && echo floating || echo tiled) on workspace 1: $w"
  cx=$("$jq_bin" -n --argjson b "$w" --argjson m "$mon" \
    '[(($b.at[0] + $b.size[0] / 2) - ($m.x + $m.w / 2)), (($b.at[1] + $b.size[1] / 2) - ($m.y + $m.h / 2))] | map(fabs) | max')
  "$jq_bin" -e -n --argjson d "$cx" '$d <= 2' > /dev/null || fail "zen $step: $n is $cx px off the output's centre"
  for o in "$@"; do
    f=$(zen_client "$step" "$o")
    echo "zen $step $o: $("$jq_bin" -c '{at, size, floating, ws: .workspace.id}' <<< "$f")"
    "$jq_bin" -e '.workspace.id == 1 and .floating == false' <<< "$f" > /dev/null || fail "zen $step: $o is not tiled on workspace 1: $f"
    zen_off_output "$f" "$mon" || fail "zen $step: $o is on the output"
  done
}

leg_zen_assert() {
  local f b b0 mon order n s
  for s in $zen_steps; do
    for f in status sidebar clients active; do
      [ -s "$(zen_file "$s" "$f")" ] || fail "zen: nothing written to $(zen_file "$s" "$f")"
    done
    echo "zen status $s: $(cat "$(zen_file "$s" status)") $("$jq_bin" -c '[.[] | select(.class | startswith("formalshell-zen-")) | [(.class | ltrimstr("formalshell-zen-")), .at[0], .at[1], .size[0], .size[1], .floating]] | sort_by(.[1])' "$(zen_file "$s" clients)")"
  done
  [ -s "$zen_monitors" ] || fail "zen: nothing written to $zen_monitors"
  mon=$("$jq_bin" -c '.[0] | {x, y, w: (.width / .scale), h: (.height / .scale)}' "$zen_monitors")

  "$jq_bin" -e '.on == false' "$(zen_file before status)" > /dev/null || fail "zen: on before enter: $(cat "$(zen_file before status)")"
  "$jq_bin" -e '.collapsed == false' "$(zen_file before sidebar)" > /dev/null || fail "zen: the sidebar started folded"
  b0=$(zen_client before b)
  "$jq_bin" -e '.floating == false and .workspace.id == 1' <<< "$b0" > /dev/null || fail "zen: b did not start tiled on workspace 1: $b0"

  # On.
  if [ "$wm_zen" = float ]; then
    "$jq_bin" -e '.on == true and .before != null' "$(zen_file on status)" > /dev/null \
      || fail "zen: not on, or no column to go back before: $(cat "$(zen_file on status)")"
  fi
  zen_holds on b "$mon"
  for n in a c; do
    f=$(zen_client on "$n")
    echo "zen on $n: $("$jq_bin" -c '{at, size, ws: .workspace.id}' <<< "$f")"
    if [ "$wm_zen" = layout ] && [ "$n" = c ]; then
      "$jq_bin" -e --argjson m "$mon" '.workspace.id == 1 and .at[0] >= $m.x + $m.w' <<< "$f" > /dev/null \
        || fail "zen: c is not off the output's right edge on workspace 1"
      continue
    fi
    "$jq_bin" -e --argjson m "$mon" '.workspace.id == 1 and .at[0] + .size[0] <= $m.x' <<< "$f" > /dev/null \
      || fail "zen: $n is not off the output's left edge on workspace 1"
  done

  # Zen goes with the focus.
  zen_holds right c "$mon" a b
  zen_holds left b "$mon" a c
  zen_holds switcher a "$mon" b c

  # Off.
  "$jq_bin" -e '.on == false' "$(zen_file off status)" > /dev/null || fail "zen: still on after exit"
  "$jq_bin" -e '.collapsed == false' "$(zen_file off sidebar)" > /dev/null || fail "zen: the sidebar stayed folded after exit"
  b=$(zen_client off b)
  echo "zen off b: $("$jq_bin" -c '{at, size, floating}' <<< "$b") (before $("$jq_bin" -c '.size' <<< "$b0"))"
  "$jq_bin" -e '.floating == false and .workspace.id == 1' <<< "$b" > /dev/null || fail "zen: b is not tiled on workspace 1 again: $b"
  "$jq_bin" -e -n --argjson b "$b" --argjson b0 "$b0" '($b.size[0] - $b0.size[0]) | fabs <= 2' > /dev/null \
    || fail "zen: b came back $("$jq_bin" -c '.size[0]' <<< "$b") wide, not $("$jq_bin" -c '.size[0]' <<< "$b0")"
  zen_on_output "$b" "$mon" || fail "zen: b is not on the output after exit"
  order=$("$jq_bin" -r '[.[] | select(.workspace.id == 1 and (.class | startswith("formalshell-zen-")))] | sort_by(.at[0]) | map(.class | ltrimstr("formalshell-zen-")) | join(" ")' "$(zen_file off clients)")
  echo "zen: tape after exit: $order"
  [ "$order" = "a b c" ] || fail "zen: b did not go back between a and c: $order"
  "$jq_bin" -e '.class == "formalshell-zen-b"' "$(zen_file off active)" > /dev/null || fail "zen: b is not focused after exit"

  # A stack: K and J walk it.
  zen_holds stack c "$mon" a b
  zen_holds up b "$mon" a c
  zen_holds down c "$mon" a b
  b=$(zen_client restacked b); c=$(zen_client restacked c)
  "$jq_bin" -e -n --argjson b "$b" --argjson c "$c" '$b.floating == false and $c.floating == false and $b.at[0] == $c.at[0] and $c.at[1] > $b.at[1]' > /dev/null \
    || fail "zen: c is not back under b in their column: b $("$jq_bin" -c '{at, floating}' <<< "$b"), c $("$jq_bin" -c '{at, floating}' <<< "$c")"

  # The zen window closing hands zen to the window the focus goes to.
  n=$("$jq_bin" -r '.class' "$(zen_file closed active)" | sed 's/^formalshell-zen-//')
  case $n in a | c) ;; *) fail "zen closed: the focus is on '$n', not a or c" ;; esac
  zen_holds closed "$n" "$mon" "$([ "$n" = a ] && echo c || echo a)"
  "$jq_bin" -e '.on == false' "$(zen_file after status)" > /dev/null || fail "zen: still on after the last exit"
  "$jq_bin" -e '.collapsed == false' "$(zen_file after sidebar)" > /dev/null || fail "zen: the sidebar stayed folded after the last exit"
  f=$(zen_client after "$n")
  echo "zen after: $n $("$jq_bin" -c '{at, size}' <<< "$f")"
  zen_on_output "$f" "$mon" || fail "zen: the tape stayed off the output after the last exit"

  # Neighbours changing under the zen window: the focus binds go by the
  # tape as it is, and come back after a reload.
  zen_holds four c "$mon" a d e
  zen_holds past-closed e "$mon" a c
  zen_holds past-moved a "$mon" e
  "$jq_bin" -e '.[] | select(.class == "formalshell-zen-c") | .workspace.id == 2' "$(zen_file past-moved clients)" > /dev/null \
    || fail "zen past-moved: c is not on workspace 2"
  zen_holds past-reload e "$mon" a
  "$jq_bin" -e '.on == false' "$(zen_file end status)" > /dev/null || fail "zen: still on after the end"

  echo "SMOKE_ZEN_ON $zen_on_png"
  echo "SMOKE_ZEN_RIGHT $zen_right_png"
  echo "SMOKE_ZEN_OFF $zen_off_png"
  echo "SMOKE_ZEN_CLOSED $zen_closed_png"
}
