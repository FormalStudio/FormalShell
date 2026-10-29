# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mirror: the launcher's camera mirror against two real V4L2 capture nodes.
# The VM has no camera, so the VM rig builds v4l2loopback (not loaded)
# and this leg loads it and feeds /dev/video10 a colour test pattern and
# /dev/video11 a GREY one, the pixel format an IR sensor advertises, each
# through a real ffmpeg writer.
#
# In order: the view opened with no /dev/video* at all (the honest No camera
# state, no device held), the module loaded and both feeds running, the view
# reopened and showing the colour camera with pixels in the feed box and the
# shell holding only that node, a real Tab key stepping to the IR camera
# (grey pixels, the other node released), IPC stepping back and forth, and
# the launcher closed with the shell holding no /dev/video node at all.
# Enumeration happens when the view is built, so the second open is what
# finds the nodes; a node appearing under an open view is Qt's own inotify
# watch and is not asserted here.
#
# The GREY feed alternates a dim frame and a near-black one, the way a
# laptop IR sensor's emitter lights every other frame. A burst of frames on
# the IR camera must never show the dark one, and must show the dim one
# levelled up: brighter and wider in spread than the raw frame, both read
# off the picture rect `mirror status` reports against the same two frames
# rendered by ffmpeg straight to PNG.
leg_mirror_flag="--mirror"
leg_mirror_order=145
leg_mirror_vm_only="it loads and unloads the v4l2loopback module with sudo"
leg_mirror_needs="convert ffmpeg jq wtype"

mirror_dir="$shot_dir/mirror"

# Odd frames lit (the pattern at 35% plus a floor), even ones nearly black.
mirror_ir_filter="format=gray,geq=lum='if(mod(N,2),lum(X,Y)*0.35+20,lum(X,Y)*0.04)'"

leg_mirror_timing() {
  leg_timing 48 76
}

leg_mirror_drive() {
  local script="$shot_dir/mirror-drive.sh" kill_script="$shot_dir/mirror-kill.sh"
  mkdir -p "$mirror_dir"
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
for f in "$mirror_dir"/feed-*.pid; do
  [ -f "\$f" ] && kill "\$(cat "\$f")" 2>/dev/null
done
sleep 0.5
sudo -n modprobe -r v4l2loopback 2>/dev/null || true
exit 0
EOF
  add_cleanup "bash $kill_script"

  write_script "$script" <<EOF
#!/usr/bin/env bash
ipc() { $ipc call "\$@"; }
# Every /dev/video node the shell itself holds open.
held() { ls -l /proc/\$(cat "$shot_dir/shell.pid")/fd 2>/dev/null | grep -o '/dev/video[0-9]*' | sort -u | tr '\n' ' '; }
snap() {
  ipc mirror status > "$mirror_dir/status-\$1.json" 2>&1
  held > "$mirror_dir/held-\$1.txt"
  "$grim_bin" "$mirror_dir/\$1.png" > /dev/null 2>&1
}
sleep 6
ls /dev/video* > "$mirror_dir/nodes-before.txt" 2>&1
ipc menu summon mirror > /dev/null 2>&1
sleep 3
snap nocamera
ipc menu close > /dev/null 2>&1
sleep 1

sudo -n modprobe v4l2loopback
# The nodes exist before udev has handed them to the video group.
for i in \$(seq 1 40); do [ -w /dev/video10 ] && [ -w /dev/video11 ] && break; sleep 0.5; done
"$ffmpeg_bin" -nostdin -loglevel error -re -f lavfi -i "testsrc2=size=640x480:rate=15" -pix_fmt yuyv422 -f v4l2 /dev/video10 2> "$mirror_dir/feed-colour.log" &
echo \$! > "$mirror_dir/feed-colour.pid"
"$ffmpeg_bin" -nostdin -loglevel error -re -f lavfi -i "testsrc2=size=640x360:rate=15" -vf "$mirror_ir_filter" -pix_fmt gray -f v4l2 /dev/video11 2> "$mirror_dir/feed-ir.log" &
echo \$! > "$mirror_dir/feed-ir.pid"
sleep 3
# Qt lists cameras again only when /dev changes. The loopback nodes appeared
# before a writer gave them a format, so touch /dev now that they have one;
# a real device is listed complete the moment its node appears.
sudo -n touch /dev/mirror-rescan
sudo -n rm -f /dev/mirror-rescan
sleep 1
held > "$mirror_dir/held-idle.txt"

ipc menu summon mirror > /dev/null 2>&1
sleep 4
snap first
"$wtype_bin" -k Tab
sleep 3
snap second
for i in \$(seq 1 10); do
  "$grim_bin" "$mirror_dir/burst-\$i.png" > /dev/null 2>&1
  sleep 0.13
done
ipc mirror previous > /dev/null 2>&1
sleep 2
snap back
ipc mirror next > /dev/null 2>&1
sleep 2
snap again
ipc mirror toggle > /dev/null 2>&1
sleep 2
snap closed
ipc mirror toggle > /dev/null 2>&1
sleep 3
snap reopened
ipc mirror close > /dev/null 2>&1
sleep 2
snap closedagain
touch "$mirror_dir/done"
EOF
  hypr_exec_once "bash $script"
}

# Standard deviation of luma and mean saturation over the middle half of the
# feed box, both 0..255 and 0..100. A flat card is ~0 on both; the colour
# test pattern is high on both; the grey one is high on luma and ~0 on
# saturation.
mirror_measure() {
  local png="$1" rect="$2" x y w h
  read -r x y w h < <(jq -r '"\(.x + .width / 4 | floor) \(.y + .height / 4 | floor) \(.width / 2 | floor) \(.height / 2 | floor)"' <<<"$rect")
  local crop="${w}x${h}+${x}+${y}"
  local sd sat
  sd=$($convert_bin "$png" -crop "$crop" +repage -colorspace Gray -format '%[fx:int(standard_deviation*255)]' info: 2>/dev/null)
  sat=$($convert_bin "$png" -crop "$crop" +repage -colorspace HSL -channel G -separate +channel -format '%[fx:int(mean*100)]' info: 2>/dev/null)
  echo "$sd $sat"
}

# Mean and standard deviation of luma, 0..255, over the middle half of a
# rect of a PNG (the whole PNG when the rect is empty).
mirror_levels() {
  local png="$1" rect="$2" crop="" x y w h
  if [ -n "$rect" ]; then
    read -r x y w h < <(jq -r '"\(.x + .width / 4 | floor) \(.y + .height / 4 | floor) \(.width / 2 | floor) \(.height / 2 | floor)"' <<<"$rect")
    crop="${w}x${h}+${x}+${y}"
  else
    crop="320x180+160+90"
  fi
  echo "$($convert_bin "$png" -crop "$crop" +repage -colorspace Gray -format '%[fx:int(mean*255)] %[fx:int(standard_deviation*255)]' info: 2>/dev/null)"
}

mirror_field() {
  jq -r "$2" "$mirror_dir/status-$1.json"
}

leg_mirror_assert() {
  local s sd sat
  [ -f "$mirror_dir/done" ] || fail "the mirror drive script never finished"
  for s in nocamera first second back again closed reopened closedagain; do
    [ -s "$mirror_dir/status-$s.json" ] || fail "no mirror status for $s"
    [ -f "$mirror_dir/$s.png" ] || fail "no mirror screenshot for $s"
    echo "SMOKE_MIRROR_$(echo "$s" | tr a-z A-Z) $mirror_dir/$s.png"
  done

  [ ! -s "$mirror_dir/feed-colour.log" ] || fail "the colour feed writer failed: $(cat "$mirror_dir/feed-colour.log")"
  [ ! -s "$mirror_dir/feed-ir.log" ] || fail "the IR feed writer failed: $(cat "$mirror_dir/feed-ir.log")"

  # A machine with no camera says so, holds nothing and draws no feed.
  grep -q 'No such file' "$mirror_dir/nodes-before.txt" \
    || fail "the rig already had /dev/video nodes before the leg loaded its own: $(cat "$mirror_dir/nodes-before.txt")"
  jq -e '.showing and .open and (.cameras | length) == 0 and .streaming == false and .hasFrame == false and .feed != null' \
    "$mirror_dir/status-nocamera.json" > /dev/null \
    || fail "the view did not report the honest no-camera state: $(cat "$mirror_dir/status-nocamera.json")"
  [ -z "$(tr -d ' \n' < "$mirror_dir/held-nocamera.txt")" ] || fail "the shell held a video node with no camera: $(cat "$mirror_dir/held-nocamera.txt")"
  read -r sd sat < <(mirror_measure "$mirror_dir/nocamera.png" "$(jq -c .feed "$mirror_dir/status-nocamera.json")")
  echo "nocamera: luma sd $sd, saturation $sat"
  [ "$sd" -lt 25 ] || fail "the no-camera feed box is not flat (luma sd $sd)"

  # Loaded and fed, but nothing opens a node until the view asks.
  [ -z "$(tr -d ' \n' < "$mirror_dir/held-idle.txt")" ] || fail "the shell held a video node while the launcher was closed: $(cat "$mirror_dir/held-idle.txt")"

  # Colour camera first, IR last, both labelled by the loopback card names.
  jq -e '.streaming and .hasFrame and .error == "" and .current == "/dev/video10"
    and (.cameras | map(.id)) == ["/dev/video10", "/dev/video11"]
    and (.cameras | map(.ir)) == [false, true]' "$mirror_dir/status-first.json" > /dev/null \
    || fail "the view did not open on the colour camera with the IR one listed after it: $(cat "$mirror_dir/status-first.json")"
  jq -e '(.cameras | map(.label)) == ["Loop Colour", "Loop IR"]' "$mirror_dir/status-first.json" > /dev/null \
    || fail "camera labels are not the devices' own names: $(cat "$mirror_dir/status-first.json")"
  [ "$(tr -d '\n' < "$mirror_dir/held-first.txt" | xargs)" = "/dev/video10" ] \
    || fail "the shell should hold only /dev/video10 on the first camera, holds: $(cat "$mirror_dir/held-first.txt")"
  read -r sd sat < <(mirror_measure "$mirror_dir/first.png" "$(jq -c .feed "$mirror_dir/status-first.json")")
  echo "first (colour): luma sd $sd, saturation $sat"
  [ "$sd" -ge 40 ] || fail "the colour feed is flat (luma sd $sd)"
  [ "$sat" -ge 15 ] || fail "the colour feed carries no colour (saturation $sat)"

  # A real Tab: the IR camera, grey pixels, the colour node released.
  jq -e '.streaming and .hasFrame and .current == "/dev/video11"' "$mirror_dir/status-second.json" > /dev/null \
    || fail "Tab did not step to the IR camera: $(cat "$mirror_dir/status-second.json")"
  [ "$(tr -d '\n' < "$mirror_dir/held-second.txt" | xargs)" = "/dev/video11" ] \
    || fail "the shell should hold only /dev/video11 on the IR camera, holds: $(cat "$mirror_dir/held-second.txt")"
  read -r sd sat < <(mirror_measure "$mirror_dir/second.png" "$(jq -c .feed "$mirror_dir/status-second.json")")
  echo "second (IR): luma sd $sd, saturation $sat"
  [ "$sd" -ge 40 ] || fail "the IR feed is flat (luma sd $sd)"
  [ "$sat" -le 5 ] || fail "the IR feed is not grey (saturation $sat)"

  # The IR camera draws through the lit-frame filter, the colour one never.
  jq -e '.irFilter == false' "$mirror_dir/status-first.json" > /dev/null || fail "the colour camera drew through the IR filter"
  jq -e '.irFilter and .picture != null' "$mirror_dir/status-second.json" > /dev/null \
    || fail "the IR camera did not draw through its lit-frame filter: $(cat "$mirror_dir/status-second.json")"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "testsrc2=size=640x360:rate=15" -vf "$mirror_ir_filter" \
    -frames:v 2 "$mirror_dir/raw-%d.png" || fail "ffmpeg could not render the raw IR frames"
  local dark_mean dark_sd lit_mean lit_sd mean sd picture i
  read -r dark_mean dark_sd < <(mirror_levels "$mirror_dir/raw-1.png" "")
  read -r lit_mean lit_sd < <(mirror_levels "$mirror_dir/raw-2.png" "")
  echo "raw IR frames: dark mean $dark_mean sd $dark_sd, lit mean $lit_mean sd $lit_sd"
  [ "$lit_mean" -gt $((dark_mean + 30)) ] || fail "the raw lit and dark frames are not apart (lit $lit_mean, dark $dark_mean)"
  picture=$(jq -c .picture "$mirror_dir/status-second.json")
  for i in $(seq 1 10); do
    [ -f "$mirror_dir/burst-$i.png" ] || fail "no IR burst frame $i"
    read -r mean sd < <(mirror_levels "$mirror_dir/burst-$i.png" "$picture")
    echo "IR burst $i: mean $mean sd $sd"
    [ "$mean" -ge $((lit_mean * 13 / 10)) ] \
      || fail "IR burst frame $i is not the lit frame levelled up (mean $mean, raw lit $lit_mean, raw dark $dark_mean)"
    [ "$sd" -ge $((lit_sd * 13 / 10)) ] || fail "IR burst frame $i is not stretched (sd $sd, raw lit sd $lit_sd)"
  done
  echo "SMOKE_MIRROR_BURST $mirror_dir/burst-5.png"

  jq -e '.current == "/dev/video10"' "$mirror_dir/status-back.json" > /dev/null || fail "mirror previous did not return to the colour camera"
  jq -e '.current == "/dev/video11"' "$mirror_dir/status-again.json" > /dev/null || fail "mirror next did not return to the IR camera"

  # Closed, the LED is off: no node held, no stream, and the toggle reopens.
  jq -e '.showing == false and .streaming == false' "$mirror_dir/status-closed.json" > /dev/null \
    || fail "the camera kept streaming after the launcher closed: $(cat "$mirror_dir/status-closed.json")"
  [ -z "$(tr -d ' \n' < "$mirror_dir/held-closed.txt")" ] || fail "the shell still holds a video node after close: $(cat "$mirror_dir/held-closed.txt")"
  jq -e '.showing and .streaming and .hasFrame and .current == "/dev/video10"' "$mirror_dir/status-reopened.json" > /dev/null \
    || fail "mirror toggle did not reopen the feed on the colour camera: $(cat "$mirror_dir/status-reopened.json")"
  jq -e '.showing == false and .streaming == false' "$mirror_dir/status-closedagain.json" > /dev/null \
    || fail "mirror close left the feed streaming: $(cat "$mirror_dir/status-closedagain.json")"
  [ -z "$(tr -d ' \n' < "$mirror_dir/held-closedagain.txt")" ] || fail "the shell still holds a video node after mirror close: $(cat "$mirror_dir/held-closedagain.txt")"
}
