# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --glass-behind: every translucent card blurring what is behind it, or
# opaque. The rig boots with no layer rules and blur off; this leg loads
# the shipped docs/examples/hyprland/formalshell.lua rules (`chrome.blur`
# true) and turns blur on, so the run reads the config the owner installs.
# A foot window full of high-contrast text covers the output, and the
# launcher, the audio panel, the notification centre, the volume OSD, the
# switcher and a sticky toast are each photographed open over it, then
# again over a flat wallpaper with the window moved to workspace 2, and
# once with nothing open in both states.
#
# What a card lets through is the difference between its two frames
# (text minus bare), set against the same difference with nothing open:
# its edge energy inside the card over the text's own. A card with
# nothing blurred behind it shows the text crisp at 1 - surfaceOpacity
# (0.15 of it); a blurred or opaque one shows next to none. The card is
# the launcher's rect off `debug dump` (its scrim covers the output), and
# for the rest the pixels its bare frame changes against the bare frame
# with nothing open, eroded past its border and casts, less its own
# content's edges.
#
# Ridden by `--no-morph` it reads the popover cards, which draw their look
# on viewport-cropped subsurfaces Hyprland 0.56 never blurs under; by
# `--light` the light cards, over a dark wallpaper rather than a bright
# one. FS_SMOKE_SCALE=1.25 runs it at g815's laptop panel scale.
leg_glass_behind_flag="--glass-behind"
leg_glass_behind_order=239
leg_glass_behind_needs="jq convert foot python3 notify-send"

glass_behind_bright="$shot_dir/glass-behind-bright.png"
glass_behind_dark="$shot_dir/glass-behind-dark.png"
glass_behind_numbers="$shot_dir/glass-behind-numbers.txt"
glass_behind_surfaces="none menu panel centre osd switcher toast"

leg_glass_behind_fixture() {
  $convert_bin -size 1920x1080 xc:'#d0d0d0' "$glass_behind_bright"
  $convert_bin -size 1920x1080 xc:'#202020' "$glass_behind_dark"
}

leg_glass_behind_timing() {
  leg_timing 30 90
}

leg_glass_behind_drive() {
  # The shipped rules, as the owner's hyprland.lua loads them.
  echo 'local chrome = { blur = true }'
  awk '/^hl.layer_rule\(\{/ { on = 1 } on { print } on && /^\}\)/ { on = 0 }' docs/examples/hyprland/formalshell.lua
  echo 'hl.config({ decoration = { blur = { enabled = true, size = 8, passes = 2, new_optimizations = true } } })'
  local script="$shot_dir/glass-behind-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# One frame per surface, \$1 naming the phase (text or bare).
shoot() {
  local p=\$1
  "$grim_bin" "$shot_dir/glass-behind-\$p-none.png" > /dev/null 2>&1
  $ipc call menu toggle > /dev/null 2>&1
  sleep 1.5
  $ipc call debug dump > "$shot_dir/glass-behind-\$p-menu.json" 2>&1
  "$grim_bin" "$shot_dir/glass-behind-\$p-menu.png" > /dev/null 2>&1
  $ipc call menu toggle > /dev/null 2>&1
  sleep 1
  $ipc call panel open audio > /dev/null 2>&1
  sleep 1.5
  "$grim_bin" "$shot_dir/glass-behind-\$p-panel.png" > /dev/null 2>&1
  $ipc call panel close > /dev/null 2>&1
  sleep 1
  $ipc call notifications showHistory > /dev/null 2>&1
  sleep 1.5
  "$grim_bin" "$shot_dir/glass-behind-\$p-centre.png" > /dev/null 2>&1
  $ipc call notifications showHistory > /dev/null 2>&1
  sleep 1
  $ipc call osd volume > /dev/null 2>&1
  sleep 0.7
  "$grim_bin" "$shot_dir/glass-behind-\$p-osd.png" > /dev/null 2>&1
  $ipc call osd close > /dev/null 2>&1
  sleep 1
  $ipc call switcher next > /dev/null 2>&1
  sleep 1
  "$grim_bin" "$shot_dir/glass-behind-\$p-switcher.png" > /dev/null 2>&1
  $ipc call switcher cancel > /dev/null 2>&1
  sleep 1
  "$notify_send_bin" -t 0 "Glass" "A card over the text"
  sleep 1.5
  "$grim_bin" "$shot_dir/glass-behind-\$p-toast.png" > /dev/null 2>&1
  $ipc call notifications clear > /dev/null 2>&1
  sleep 1.5
}
sleep 2
wp="$glass_behind_bright"
$ipc call theme status 2>/dev/null | jq -e '.mode == "light"' > /dev/null && wp="$glass_behind_dark"
$ipc call wallpaper set "\$wp" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-glass sh -c 'for i in \\\$(seq 1 3000); do printf \"WMWMWM ||| ### IIII mmmm \"; done; sleep 300']==])" > /dev/null 2>&1
sleep 6
shoot text
"$hyprctl_bin" dispatch "hl.dsp.window.move({ window = \"class:formalshell-glass\", workspace = 2, follow = false })" > /dev/null 2>&1
sleep 1
shoot bare
touch "$shot_dir/glass-behind-done"
EOF
  hypr_exec_once "bash $script"
}

leg_glass_behind_assert() {
  [ -e "$shot_dir/glass-behind-done" ] || fail "glass-behind: the drive never finished"
  local s p
  for s in $glass_behind_surfaces; do
    for p in text bare; do
      [ -s "$shot_dir/glass-behind-$p-$s.png" ] || fail "glass-behind: glass-behind-$p-$s.png missing or empty"
    done
  done
  for s in $glass_behind_surfaces; do
    for p in text bare; do
      echo "SMOKE_GLASS_BEHIND_$(echo "$p-$s" | tr '[:lower:]-' '[:upper:]_') $shot_dir/glass-behind-$p-$s.png"
    done
  done
  local dump="$shot_dir/glass-behind-text-menu.json"
  jq -e '.theme.blur == true and .theme.surfaceOpacity < 1' "$dump" > /dev/null \
    || fail "glass-behind: blur on did not make the cards translucent: $(jq -c '.theme | {blur, surfaceOpacity}' "$dump")"
  local box
  box=$(jq -r '.modals[] | select(.namespace == "formalshell:menu") | .card | "\(.x) \(.y) \(.width) \(.height)"' "$dump")
  [ -n "$box" ] || fail "glass-behind: no launcher card in the dump"
  # The dump's rect is in logical pixels, grim's frame in buffer pixels.
  local scale="${FS_SMOKE_SCALE:-1}"
  "$python3_bin" - "$shot_dir" "$scale" "$box" $glass_behind_surfaces > "$glass_behind_numbers" <<'PY' \
    || fail "glass-behind: $(cat "$glass_behind_numbers")"
import sys
from PIL import Image, ImageChops, ImageFilter, ImageStat
d, scale, box = sys.argv[1], float(sys.argv[2]), [int(v) for v in sys.argv[3].split()]
names = sys.argv[5:]
L = lambda p, s: Image.open(f"{d}/glass-behind-{p}-{s}.png").convert("L")
t0, b0 = L("text", "none"), L("bare", "none")
text = ImageChops.difference(t0, b0).filter(ImageFilter.FIND_EDGES)
bad = []
for s in names:
    ts, bs = L("text", s), L("bare", s)
    if s == "menu":
        x, y, w, h = (round(v * scale) for v in box)
        mask = Image.new("L", ts.size, 0)
        mask.paste(255, (x + 20, y + 20, x + w - 20, y + h - 20))
    else:
        # Where the card covers the flat wallpaper, short of its border and
        # its casts.
        mask = ImageChops.difference(bs, b0).point(lambda v: 255 if v > 80 else 0).filter(ImageFilter.MinFilter(15))
    # Less the card's own content, whose antialiased edges composite
    # differently over the two backgrounds whatever is behind them.
    content = bs.filter(ImageFilter.FIND_EDGES).point(lambda v: 255 if v > 16 else 0).filter(ImageFilter.MaxFilter(7))
    mask = ImageChops.subtract(mask, content)
    area = ImageStat.Stat(mask).mean[0] / 255 * mask.size[0] * mask.size[1]
    if area < 500:
        bad.append(f"{s}: no card found ({area:.0f} px)")
        continue
    leak = ImageChops.difference(ts, bs).filter(ImageFilter.FIND_EDGES)
    through = ImageStat.Stat(leak, mask).mean[0]
    behind = ImageStat.Stat(text, mask).mean[0]
    ratio = through / behind if behind else float("nan")
    print(f"{s} area={area:.0f} through={through:.2f} behind={behind:.2f} ratio={ratio:.3f}")
    if not behind > 5:
        bad.append(f"{s}: no text behind the card (edge energy {behind:.2f})")
    elif ratio > 0.06:
        bad.append(f"{s}: the text behind shows through sharp ({ratio:.3f} of it)")
print("; ".join(bad))
sys.exit(1 if bad else 0)
PY
  cat "$glass_behind_numbers"
}
