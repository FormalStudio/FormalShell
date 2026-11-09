# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --lights: LightsService against PATH-shimmed `asusctl` and `busctl`. The
# VM has no ASUS keyboard and no asusd, so the shims stand in for both: the
# asusctl shim records its argv and writes what asusd would keep, the busctl
# shim answers the four Aura properties off those files in busctl's own
# text form and hands every other call to the real busctl.
#
# The shimmed keyboard starts on static Flexoki blue, the g815's reading,
# with the shipped default source (wallpaper), so the first write has to be
# the theme's primary. Then effect, speed, a custom colour, the toggle both
# ways, a refused effect and the source back to the wallpaper, each read
# off `lights status` and the shim's call log, and the launcher's effect
# level photographed with its tick on Breathe.
leg_lights_flag="--lights"
leg_lights_order=216

lights_shim_dir="$shot_dir/lights-shim"
lights_aura_dir="$shot_dir/lights-aura"
lights_calls_path="$shot_dir/lights-calls.log"
lights_menu_png="$shot_dir/lights-menu.png"
lights_refused_path="$shot_dir/lights-refused.txt"

leg_lights_fixture() {
  local real_busctl
  real_busctl=$(command -v busctl) || fail "no busctl to forward to"
  mkdir -p "$lights_shim_dir" "$lights_aura_dir"
  echo 0 > "$lights_aura_dir/mode"
  echo "67 133 190" > "$lights_aura_dir/rgb"
  echo Med > "$lights_aura_dir/speed"
  echo 3 > "$lights_aura_dir/brightness"
  : > "$lights_calls_path"

  cat > "$lights_shim_dir/asusctl" <<EOF
#!/usr/bin/env bash
echo "\$*" >> "$lights_calls_path"
d="$lights_aura_dir"
if [ "\$1 \$2" = "leds set" ]; then
  case "\$3" in off) echo 0 ;; low) echo 1 ;; med) echo 2 ;; high) echo 3 ;; *) exit 1 ;; esac > "\$d/brightness"
  exit 0
fi
[ "\$1 \$2" = "aura effect" ] || exit 1
case "\$3" in
  static) m=0 ;; breathe) m=1 ;; rainbow-cycle) m=2 ;; rainbow-wave) m=3 ;; stars) m=4 ;; rain) m=5 ;;
  highlight) m=6 ;; laser) m=7 ;; ripple) m=8 ;; pulse) m=10 ;; comet) m=11 ;; flash) m=12 ;; *) exit 1 ;;
esac
echo \$m > "\$d/mode"
shift 3
while [ \$# -gt 0 ]; do
  case "\$1" in
    -c|--colour) h="\$2"; printf '%d %d %d\n' 0x\${h:0:2} 0x\${h:2:2} 0x\${h:4:2} > "\$d/rgb"; shift 2 ;;
    --speed) case "\$2" in low) echo Low ;; med) echo Med ;; high) echo High ;; esac > "\$d/speed"; shift 2 ;;
    *) shift 2 ;;
  esac
done
EOF
  cat > "$lights_shim_dir/busctl" <<EOF
#!/usr/bin/env bash
case " \$* " in
  *" xyz.ljones.Asusd "*) ;;
  *) exec "$real_busctl" "\$@" ;;
esac
d="$lights_aura_dir"
case " \$* " in
  *" tree "*) printf '/\n/xyz\n/xyz/ljones\n/xyz/ljones/aura\n/xyz/ljones/aura/19b6_2_4\n' ;;
  *" LedMode ") echo "u \$(cat "\$d/mode")" ;;
  *" Brightness ") echo "u \$(cat "\$d/brightness")" ;;
  *" SupportedBasicModes ") echo "au 12 0 1 2 3 4 5 6 7 8 10 11 12" ;;
  *" LedModeData ") echo "(uu(yyy)(yyy)ss) \$(cat "\$d/mode") 0 \$(cat "\$d/rgb") 0 0 0 \"\$(cat "\$d/speed")\" \"Right\"" ;;
  *) exit 1 ;;
esac
EOF
  chmod +x "$lights_shim_dir/asusctl" "$lights_shim_dir/busctl"
  # Hyprland and everything it spawns inherit this PATH (airplay.sh has why).
  export PATH="$lights_shim_dir:$PATH"
}

leg_lights_timing() {
  leg_timing 50
}

leg_lights_drive() {
  local script="$shot_dir/lights-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
ipc() { $ipc call lights "\$@"; }
st() { ipc status > "$shot_dir/lights-status-\$1.json" 2>&1; }
# Past the probe and the palette's own settle timer.
st 1
ipc effect breathe > /dev/null 2>&1
ipc speed high > /dev/null 2>&1
sleep 1.5
st 2
ipc color '#FF0000' > /dev/null 2>&1
sleep 1.5
st 3
$ipc call menu summon lights.effect > /dev/null 2>&1
sleep 1.5
"$grim_bin" "$lights_menu_png" > /dev/null 2>&1
$ipc call menu status > "$shot_dir/lights-menu-status.json" 2>&1
$ipc call menu close > /dev/null 2>&1
ipc toggle > /dev/null 2>&1
sleep 1.5
st 4
ipc toggle > /dev/null 2>&1
sleep 1.5
st 5
ipc effect disco > "$lights_refused_path" 2>&1
ipc source wallpaper > /dev/null 2>&1
sleep 1.5
st 6
EOF
  hypr_exec_once "bash $script"
}

leg_lights_assert() {
  local s i
  for i in 1 2 3 4 5 6; do
    s="$shot_dir/lights-status-$i.json"
    [ -s "$s" ] || fail "no lights status $i"
    echo "SMOKE_LIGHTS_STATUS_$i $s"
  done
  s="$shot_dir/lights-status-1.json"
  jq -e '.available and .on and .effect == "static" and .source == "wallpaper"
    and (.effects | length) == 12 and .color == .paletteColor and .color != "4385be"' "$s" > /dev/null \
    || fail "startup did not repaint the static effect with the palette: $(cat "$s")"
  jq -e '.effect == "breathe" and .speed == "high" and .color == .paletteColor' "$shot_dir/lights-status-2.json" > /dev/null \
    || fail "effect/speed did not land: $(cat "$shot_dir/lights-status-2.json")"
  jq -e '.effect == "breathe" and .source == "custom" and .color == "ff0000" and .customColor == "ff0000"' "$shot_dir/lights-status-3.json" > /dev/null \
    || fail "a custom color did not switch the source: $(cat "$shot_dir/lights-status-3.json")"
  jq -e '.on == false and .brightness == 0' "$shot_dir/lights-status-4.json" > /dev/null \
    || fail "toggle did not turn the lights off: $(cat "$shot_dir/lights-status-4.json")"
  jq -e '.on and .brightness == 3' "$shot_dir/lights-status-5.json" > /dev/null \
    || fail "toggle did not restore the level: $(cat "$shot_dir/lights-status-5.json")"
  jq -e '.source == "wallpaper" and .effect == "breathe" and .color == .paletteColor' "$shot_dir/lights-status-6.json" > /dev/null \
    || fail "source wallpaper did not repaint: $(cat "$shot_dir/lights-status-6.json")"
  grep -q '^error: refused' "$lights_refused_path" || fail "an unknown effect was not refused: $(cat "$lights_refused_path")"
  cat "$lights_calls_path"
  grep -q '^aura effect breathe --colour ff0000 --colour2 ff0000 --speed high$' "$lights_calls_path" \
    || fail "asusctl never got the custom breathe"
  grep -q '^leds set off$' "$lights_calls_path" || fail "asusctl never got leds set off"
  grep -q '^leds set high$' "$lights_calls_path" || fail "asusctl never got leds set high"
  [ -f "$lights_menu_png" ] || fail "no lights menu screenshot"
  jq -e '.level == "lights.effect" and (.checked | index("lights.effect.breathe"))' "$shot_dir/lights-menu-status.json" > /dev/null \
    || fail "the effect level is not up with its tick on Breathe: $(cat "$shot_dir/lights-menu-status.json")"
  echo "SMOKE_LIGHTS_MENU $lights_menu_png"
}
