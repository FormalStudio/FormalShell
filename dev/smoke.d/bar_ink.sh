# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-ink: each cell of the wingpanel band reading its ink off its own
# stretch of the wallpaper (`barpaint::cell`). One wallpaper, black down
# the left half and white down the right, under the default
# `transparent` style: the band as a whole is bare (mean 127, light ink),
# and the claim is that the cells over the white half do not wear it.
# Both halves of the claim: `bar paint`'s `cells` naming light ink for
# every cell whose rect is inside the black half and dark ink for every
# cell inside the white half, and the frame, read over one whole cell on
# each side off `bar room`'s rects: white glyph pixels over the black half
# and dark ones (black at 0.65 over white lands at 89) over the white.
# A cell straddling the seam reads whichever its own mean says and is
# skipped here.
#
# Any other --bar-style gives the band a fill and one ink across, so the
# leg refuses one.
leg_bar_ink_flag="--bar-ink"
leg_bar_ink_order=102
leg_bar_ink_needs="convert jq"

bar_ink_wp="$shot_dir/bar-ink.png"
bar_ink_png="$shot_dir/bar-ink-band.png"
bar_ink_json="$shot_dir/bar-ink.json"
bar_ink_room_json="$shot_dir/bar-ink-room.json"

leg_bar_ink_validate() {
  if leg_on bar_style && [ "$(leg_arg bar_style)" != transparent ]; then
    echo "usage: --bar-ink reads each cell's own stretch, which only --bar-style transparent does" >&2
    exit 1
  fi
}

leg_bar_ink_fixture() {
  $convert_bin -size 1920x1080 xc:black -fill white \
    -draw "rectangle 960,0 1919,1079" "$bar_ink_wp"
}

leg_bar_ink_timing() {
  leg_timing 20 60
}

leg_bar_ink_drive() {
  local script="$shot_dir/bar-ink-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
sleep 5
$ipc call wallpaper set "$bar_ink_wp" > /dev/null 2>&1
sleep 6
$ipc call bar paint > "$bar_ink_json" 2>&1
$ipc call bar room > "$bar_ink_room_json" 2>&1
"$grim_bin" "$bar_ink_png" > /dev/null 2>&1
EOS
  hypr_exec_once "bash $script"
}

# The brightest and darkest red sample inside one rect of the frame, 0..255.
# Every ink here is white or black, so one channel is the whole answer.
_bar_ink_extremes() {
  $convert_bin "$1" -crop "$2" +repage \
    -format '%[fx:int(maxima.r*255+0.5)] %[fx:int(minima.r*255+0.5)]' info: 2>/dev/null
}

leg_bar_ink_assert() {
  local band along cells name paint x width side want checked=0
  [ -s "$bar_ink_json" ] || fail "no bar paint reply produced at $bar_ink_json"
  [ -s "$bar_ink_room_json" ] || fail "no bar room reply produced at $bar_ink_room_json"
  [ -f "$bar_ink_png" ] || fail "no frame produced at $bar_ink_png"
  band=$("$jq_bin" -r '.[0].paint.paint // "none"' "$bar_ink_json")
  along=$("$jq_bin" -r '.[0].along' "$bar_ink_room_json")
  echo "band=$band along=$along $("$jq_bin" -c '.[0].paint.cells' "$bar_ink_json")"
  [ "$band" = light ] || fail "the band reports '$band' over a half-and-half wallpaper, not the bare light paint"
  [ "${along:-0}" -gt 0 ] || fail "bar room reports no length"

  # Every whole cell clear of the seam: the shell's own answer for it, then
  # its glyphs in the frame. The seam is the strip's midpoint, since the
  # wallpaper covers the output and the strip runs its full length.
  local left_seen=0 right_seen=0 rect extremes max min
  while IFS=$'\t' read -r name x width rect; do
    if [ $((x + width)) -le $((along / 2)) ]; then
      side=left; want=light
    elif [ "$x" -ge $((along / 2)) ]; then
      side=right; want=dark
    else
      echo "$name straddles the seam, skipped"
      continue
    fi
    paint=$("$jq_bin" -r --arg n "$name" '[.[0].paint.cells[] | select(.name == $n)][0].paint // "none"' "$bar_ink_json")
    [ "$paint" = "$want" ] || fail "$name on the $side half reports '$paint' ink, not $want"
    extremes=$(_bar_ink_extremes "$bar_ink_png" "$rect")
    max=${extremes% *}; min=${extremes#* }
    echo "$name ($side, $rect): paint=$paint max=$max min=$min"
    case $side in
      left) [ "${max:-0}" -ge 200 ] || fail "$name over the black half drew no light glyph (brightest sample $max)"; left_seen=1 ;;
      right) [ "${min:-255}" -le 110 ] || fail "$name over the white half drew no dark glyph (darkest sample $min)"; right_seen=1 ;;
    esac
    checked=$((checked + 1))
  done < <("$jq_bin" -r '.[0].cells[] | select(.whole) | [.name, .x, .width, "\(.width)x\(.height)+\(.x)+\(.y)"] | @tsv' "$bar_ink_room_json")
  [ "$left_seen" = 1 ] || fail "no whole cell on the black half to read"
  [ "$right_seen" = 1 ] || fail "no whole cell on the white half to read"

  echo "SMOKE_BAR_INK ok band=$band cells=$checked"
  echo "SMOKE_BAR_INK_BAND $bar_ink_png"
}
