# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-preview: the preview pane (launcher spec §7), read off `menu
# status` (`cardWidth`, `pane`, `paneBox`, `drawn`) and `debug dump`'s
# `captures`. The leg stages its own files: Documents and Pictures named in
# user-dirs.dirs, a text file whose name carries a space (percent-encoded in
# recently-used.xbel), a magenta PNG, and a folder holding a sub-folder, two
# files and a hidden one, all three listed in the xbel. The Preview Probe
# app opens foot windows of its own app id; two are up before the first
# open.
#
# The card is 640 with no pane on the app, and → on it lists its two
# windows with the pane holding one captured frame: exactly one launcher
# capture session in `debug dump`, on the selected window, moving to the
# second with Down, none once ← leaves for the app row and none after the
# card closes. A root search reaches the text file in the same 640 card, its first
# line in the pane, the folder with its entry count and names, and the
# PNG with its picture decoded; → on the folder lists it folders first
# with the hidden entry only after typing `.`. A clipboard image keeps 640
# with the pane carrying the capture's own colour, and `menu summon files`
# lists the user directories and the recent files. The app, a window, the
# text file and the folder are photographed in dark, then in light.
leg_launcher_preview_flag="--launcher-preview"
leg_launcher_preview_order=32
leg_launcher_preview_needs="jq wtype foot convert wl-copy"

launcher_preview_app="apps.formalshell-preview-probe"
launcher_preview_first="launcher preview first line"

leg_launcher_preview_fixture() {
  local docs="$iso_home/Documents" pics="$iso_home/Pictures"
  mkdir -p "$iso_home/.local/share/applications" "$iso_home/.config" "$docs/preview-folder/gamma" "$pics"
  cat > "$iso_home/.local/share/applications/formalshell-preview-probe.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Preview Probe
Exec=$foot_bin --app-id=formalshell-preview-probe sleep 300
EOF
  printf 'XDG_DOCUMENTS_DIR="$HOME/Documents"\nXDG_PICTURES_DIR="$HOME/Pictures"\nXDG_DESKTOP_DIR="$HOME"\n' > "$iso_home/.config/user-dirs.dirs"
  { echo "$launcher_preview_first"; seq 2 80 | sed 's/^/line /'; } > "$docs/preview note.txt"
  printf 'a\n' > "$docs/preview-folder/alpha.txt"
  printf 'b\n' > "$docs/preview-folder/beta.txt"
  printf 'h\n' > "$docs/preview-folder/.hidden-entry"
  $convert_bin -size 320x180 xc:'#e0218a' "$pics/preview-swatch.png"
  $convert_bin -size 320x180 xc:'#21a0e0' "$shot_dir/launcher-preview-clip.png"
  cat > "$iso_home/.local/share/recently-used.xbel" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks" xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file://$docs/preview%20note.txt" added="2026-10-29T10:00:00.000000Z" modified="2026-10-29T12:00:00.000000Z" visited="2026-10-29T12:00:00.000000Z"/>
  <bookmark href="file://$pics/preview-swatch.png" added="2026-10-29T10:00:00Z" modified="2026-10-29T11:00:00Z" visited="2026-10-29T11:00:00Z"/>
  <bookmark href="file://$docs/preview-folder" added="2026-10-29T10:00:00Z" modified="2026-10-29T10:30:00Z" visited="2026-10-29T10:30:00Z"/>
  <bookmark href="file://$docs/gone.txt" added="2026-10-29T10:00:00Z" modified="2026-10-29T13:00:00Z" visited="2026-10-29T13:00:00Z"/>
</xbel>
EOF
}

leg_launcher_preview_timing() {
  leg_timing 80 130
}

leg_launcher_preview_drive() {
  local script="$shot_dir/launcher-preview-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$shot_dir/launcher-preview-\$1.json" 2>&1; }
dump() { call debug dump > "$shot_dir/launcher-preview-dump-\$1.json" 2>&1; }
key() { "$wtype_bin" "\$@"; sleep 0.6; }
shoot() { "$grim_bin" "$shot_dir/launcher-preview-\$1.png" > /dev/null 2>&1; }
seek() { call menu summon "" > /dev/null 2>&1; sleep 0.4; call menu filter "\$1" > /dev/null 2>&1; sleep 1.5; }
run() {
  local mode=\$1
  seek "Preview Probe"
  status "app-\$mode"
  dump "app-\$mode"
  shoot "app-\$mode"
  key -k Right
  sleep 1.5
  status "window-\$mode"
  dump "window-\$mode"
  shoot "window-\$mode"
  seek "preview note"
  status "text-\$mode"
  shoot "text-\$mode"
  seek "preview-folder"
  status "folder-\$mode"
  shoot "folder-\$mode"
  call menu close > /dev/null 2>&1
  sleep 1
}
sleep 4
for i in 1 2; do
  "$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-preview-probe sleep 300]==])" > /dev/null 2>&1
done
sleep 3

run dark
seek "Preview Probe"
key -k Right
sleep 1.5
dump into-1
key -k Down
sleep 1.5
status into-2
dump into-2
key -k Left
sleep 1
status left
dump left
seek "Preview Probe"
key -k Right
sleep 1.5
dump again
call menu close > /dev/null 2>&1
sleep 1.5
dump closed

seek "preview-folder"
key -k Right
sleep 1
status folder-into
shoot folder-into-dark
key .
sleep 0.8
status folder-hidden
key -k BackSpace
sleep 0.4
key -k Right
sleep 1
key -k Left
sleep 1
status folder-back
call menu close > /dev/null 2>&1

seek "preview-swatch"
sleep 1
status image-dark
shoot image-dark

"$wl_copy_bin" --type image/png < "$shot_dir/launcher-preview-clip.png"
sleep 2
call menu summon clipboard > /dev/null 2>&1
sleep 2.5
status clip-dark
shoot clip-dark
call menu summon files > /dev/null 2>&1
sleep 1.5
status files
call menu close > /dev/null 2>&1
sleep 1

call theme mode light > /dev/null 2>&1
sleep 2
run light
EOF
  hypr_exec_once "bash $script"
}

launcher_preview_expect() {
  local f="$shot_dir/launcher-preview-$1.json"
  [ -s "$f" ] || fail "launcher-preview: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "launcher-preview $1: $3, got $("$jq_bin" -c '{isOpen, level, cursorId, cardWidth, pane, paneBox, drawn: [.drawn[]?.label][:8]}' "$f")"
}

# The launcher's capture sessions in one `debug dump`, as a JSON array.
launcher_preview_sessions() {
  "$jq_bin" -c '.captures.launcher' "$shot_dir/launcher-preview-dump-$1.json"
}

# The mean colour of a 24px square at the centre of the pane's picture, as
# `r g b` in 0..255, off the frame the step took.
launcher_preview_colour() {
  local png="$shot_dir/launcher-preview-$1.png" box x y w h
  box=$("$jq_bin" -r '.paneBox | "\(.x) \(.y) \(.width) \(.height)"' "$shot_dir/launcher-preview-$1.json")
  read -r x y w h <<< "$box"
  # The picture hangs from the pane's top, fitted to its width.
  $convert_bin "$png" -crop "24x24+$((x + w / 2 - 12))+$((y + 40))" +repage \
    -format '%[fx:int(255*mean.r)] %[fx:int(255*mean.g)] %[fx:int(255*mean.b)]' info:
}

leg_launcher_preview_assert() {
  local mode
  for mode in dark light; do
    launcher_preview_expect "app-$mode" ".cursorId == \"$launcher_preview_app\" and .pane == null and .cardWidth == 640" "the app is not 640 with no pane"
    launcher_preview_expect "window-$mode" '.pane.kind == "window" and .pane.picture and .cardWidth == 640 and (.ids | length) == 2' "→ on the app did not show a window's captured frame"
    launcher_preview_expect "text-$mode" ".pane.kind == \"file\" and .pane.lines[0] == \"$launcher_preview_first\" and (.pane.lines | length) == 40 and .cardWidth == 640" "the text file's pane does not hold its first 40 lines"
    launcher_preview_expect "folder-$mode" '.pane.kind == "folder" and .pane.count == 3 and .pane.names == ["gamma", "alpha.txt", "beta.txt"]' "the folder's pane does not name its three visible entries"
  done
  local card
  card=$("$jq_bin" '[.modals[] | select(.namespace == "formalshell:menu" and .open) | .card.width] | first' "$shot_dir/launcher-preview-dump-app-dark.json")
  [ "$card" = 640 ] || fail "launcher-preview: the card on the app settled at $card, not 640"
  card=$("$jq_bin" '[.modals[] | select(.namespace == "formalshell:menu" and .open) | .card.width] | first' "$shot_dir/launcher-preview-dump-window-dark.json")
  [ "$card" = 640 ] || fail "launcher-preview: the pane widened the card on a window to $card, not 640"
  echo "SMOKE_LAUNCHER_PREVIEW 640 on the app and on a window, the text file's 40 lines and the folder's names"

  local first second
  first=$("$jq_bin" -r '.pane.id // ""' "$shot_dir/launcher-preview-window-dark.json")
  second=$("$jq_bin" -r '.pane.id // ""' "$shot_dir/launcher-preview-into-2.json")
  [ "$(launcher_preview_sessions window-dark)" = "[\"${first#window:}\"]" ] || fail "launcher-preview: the shown window did not hold exactly its own session: $(launcher_preview_sessions window-dark)"
  [ "$(launcher_preview_sessions into-2)" = "[\"${second#window:}\"]" ] && [ "$first" != "$second" ] \
    || fail "launcher-preview: Down did not move the one session to the second window: $(launcher_preview_sessions into-2)"
  launcher_preview_expect left '.level == null and .pane == null and ([.drawn[].label] | index("Apps") != null)' "← did not leave the windows for the root"
  [ "$(launcher_preview_sessions left)" = "[]" ] || fail "launcher-preview: a session outlived the window leaving the pane: $(launcher_preview_sessions left)"
  [ "$("$jq_bin" '.captures.launcher | length' "$shot_dir/launcher-preview-dump-again.json")" = 1 ] || fail "launcher-preview: → again did not capture one window"
  [ "$(launcher_preview_sessions closed)" = "[]" ] || fail "launcher-preview: a session outlived the card: $(launcher_preview_sessions closed)"
  [ "$("$jq_bin" -c '.captures.switcher' "$shot_dir/launcher-preview-dump-closed.json")" = "[]" ] || fail "launcher-preview: the switcher holds a capture with its card shut"
  echo "SMOKE_LAUNCHER_PREVIEW one session on the shown window, moved by Down, none after ← and after close"

  launcher_preview_expect folder-back '[.drawn[].label] == ["gamma", "alpha.txt", "beta.txt"] and .token[-1] == "preview-folder"' "← out of the subfolder did not list the folder again"
  launcher_preview_expect folder-into '[.drawn[].label] == ["gamma", "alpha.txt", "beta.txt"] and .token[-1] == "preview-folder"' "→ on the folder did not list it folders first without the hidden entry"
  launcher_preview_expect folder-hidden '[.drawn[].label] | index(".hidden-entry") != null' "typing . did not list the hidden entry"
  echo "SMOKE_LAUNCHER_PREVIEW → listed the folder, its hidden entry only after ."

  launcher_preview_expect image-dark '.pane.kind == "file" and .pane.image and .pane.picture and .cardWidth == 640' "the PNG's pane did not decode its picture"
  local rgb r g b
  echo "SMOKE_LAUNCHER_PREVIEW_IMAGE $shot_dir/launcher-preview-image-dark.png"
  rgb=$(launcher_preview_colour image-dark)
  read -r r g b <<< "$rgb"
  [ "$r" -gt 180 ] && [ "$g" -lt 90 ] && [ "$b" -gt 90 ] || fail "launcher-preview: the PNG's pane is not its magenta, read $rgb"
  launcher_preview_expect clip-dark '.level == "clipboard" and .pane.kind == "clipboard" and .pane.picture and .cardWidth == 640' "the clipboard image is not 640 with its picture"
  rgb=$(launcher_preview_colour clip-dark)
  read -r r g b <<< "$rgb"
  [ "$r" -lt 90 ] && [ "$g" -gt 120 ] && [ "$b" -gt 180 ] || fail "launcher-preview: the clipboard image's pane is not its blue, read $rgb"
  echo "SMOKE_LAUNCHER_PREVIEW the PNG and the clipboard image in the pane at their own colour"

  launcher_preview_expect files '.level == "files" and ([.drawn[].label] | (index("Documents") != null and index("Pictures") != null and index("preview note.txt") != null and index("preview-swatch.png") != null and index("preview-folder") != null and index("gone.txt") == null))' "the Files mode does not list the user directories and the recent files that exist"
  echo "SMOKE_LAUNCHER_PREVIEW the Files mode lists the user directories and the recent files"

  local shot
  for mode in dark light; do
    for shot in app window text folder; do
      [ -s "$shot_dir/launcher-preview-$shot-$mode.png" ] || fail "launcher-preview: no $shot-$mode frame"
      echo "SMOKE_LAUNCHER_PREVIEW_SHOT $shot_dir/launcher-preview-$shot-$mode.png"
    done
  done
  for shot in folder-into-dark image-dark clip-dark; do
    echo "SMOKE_LAUNCHER_PREVIEW_SHOT $shot_dir/launcher-preview-$shot.png"
  done
}
