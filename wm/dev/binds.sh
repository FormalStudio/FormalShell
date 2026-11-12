#!/bin/bash
# Every bind of the settings that runs a command, pressed on the rig with a
# recorder standing where the command is: what ran is what hyprland.lua
# names. With FWM_SHELL naming a shell, the binds a locked session keeps.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
settings="$XDG_CONFIG_HOME/formalwm/settings.json"

step "a recorder in place of formalshell, wpctl, helium, ghostty and systemctl"
[ -z "${FWM_SHELL:-}" ] || sleep 8
printf '#!/bin/sh\necho "$(basename "$0") $*" >> "%s/spied"\n' "$run" > "$run/spy"; chmod +x "$run/spy"
for name in formalshell wpctl helium ghostty systemctl; do ln -s spy "$run/$name"; done
sed -i -e "s#/opt/fspec/current/bin/formalshell#$run/formalshell#g" -e "s#/usr/bin/wpctl#$run/wpctl#g" \
  -e "s#/opt/fspec/current/bin/helium#$run/helium#g" -e "s#\"ghostty\"#\"$run/ghostty\"#g" \
  -e "s#&& systemctl suspend#\&\& $run/systemctl suspend#g" "$settings"
# Two more binds on Alt let go, one without `passthrough` and one that has
# it on a bare Alt_L: a chord that used Alt takes both, Alt on its own
# neither.
printf '    {"chord": "ALT+Alt_L", "action": "spawn", "args": ["%s/formalshell", "plain"], "release": true},\n' "$run" > "$run/binds"
printf '    {"chord": "Alt_L", "action": "spawn", "args": ["%s/formalshell", "bare"], "release": true, "passthrough": true},\n' "$run" >> "$run/binds"
sed -i "/\"binds\": \[/r $run/binds" "$settings"
( ipc state.get --listen 2 | jq -c 'select(.event != null and .event != "state.changed")' ) & listener=$!
sleep 0.4; printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; wait "$listener"
ipc binds.list | jq -c 'length'
: > "$run/spied"

pressed() { printf '%-26s %s\n' "$1" "$(tr '\n' '|' < "$run/spied")"; : > "$run/spied"; }
press() { key "$1" 0.6; pressed "$1"; }

step "the shell's binds, the screenshots, the media keys, the browser and the Copilot key"
for chord in Super+space Super+period Super+ntilde Super+plus Super+Shift+t Super+Shift+e Print Super+Shift+s \
  XF86AudioMute XF86AudioMicMute XF86AudioPlay XF86AudioPause XF86AudioNext XF86AudioPrev \
  Super+Shift+F23 Super+b; do press "$chord"; done

step "Alt+Tab steps the switcher, Shift reverses, letting Alt go commits, with no key after it"
key Alt+Tab 1; pressed Alt+Tab
key Alt+Shift+Tab 1; pressed Alt+Shift+Tab
wtype -M alt -k Tab -s 300 -k Tab -s 300 -m alt; sleep 0.8; pressed "Alt+Tab, Tab"
"$SHOJI_BIN" --key Alt+Tab >/dev/null; sleep 1; pressed "--key Alt+Tab"
"$SHOJI_BIN" --key Alt_L >/dev/null; sleep 1; pressed "--key Alt_L, alone"

step "while Alt is held the pointer is free: another window hears it, and a click focuses it"
win left > "$run/left.out"; sleep 1.2; win right > "$run/right.out"; sleep 1.2; focus_app
wtype -M alt -k Tab -s 2500 -m alt & held=$!
sleep 0.7; before=$(grep -c enter "$run/left.out")
hand at $(centre left) wait 300 press release; sleep 0.5
echo "left heard the pointer: $(($(grep -c enter "$run/left.out") - before)) time, focus: $(focus_app), so far: $(tr '\n' '|' < "$run/spied")"
wait "$held"; sleep 0.6; pressed "Alt let go"
for app in left right; do ipc window.close "{\"id\":\"$(id_of "$app")\"}" >/dev/null; done; sleep 0.8

step "the keys that repeat while held, and one that does not"
for chord in XF86AudioRaiseVolume XF86AudioLowerVolume XF86MonBrightnessUp XF86MonBrightnessDown XF86AudioMute; do
  hold "$chord" 900; sleep 1.2
  printf '%-26s %s times: %s\n' "$chord" "$(wc -l < "$run/spied")" "$(head -1 "$run/spied")"; : > "$run/spied"
done

step "Sleep locks and then suspends, through sh"
if ipc binds.list | jq -e --arg run "$run" '[.[] | select(.title == "Sleep") | .args[0] | contains("&& " + $run + "/systemctl suspend")] == [true]' >/dev/null; then
  press Super+Shift+Escape
else
  echo "not pressed: the bind would reach the real systemctl"
fi

step "SUPER+Return opens the host's terminal, SUPER+Q closes the focused window"
key Super+Return 1.5; ipc state.get | jq -c '[.windows[] | .appId]'
key Super+q 1.2; ipc state.get | jq -c '[.windows[] | .appId]'

if [ -n "${FWM_SHELL:-}" ]; then
  step "a second bind on the volume key, not marked locked: both run"
  ln -s spy "$run/twin"
  sed -i "/\"chord\": \"XF86AudioRaiseVolume\"/a {\"chord\": \"XF86AudioRaiseVolume\", \"action\": \"spawn\", \"args\": [\"$run/twin\"]}," "$settings"
  printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1
  : > "$run/spied"; press XF86AudioRaiseVolume
  hold XF86AudioRaiseVolume 900; sleep 1.2
  echo "held: the volume $(grep -c '^wpctl' "$run/spied") times, its twin, which does not repeat, $(grep -c '^twin' "$run/spied")"; : > "$run/spied"

  step "locked: the binds marked locked still run, the others do not, the volume key's second bind among them"
  "$FWM_SHELL" ipc --any-display call lock lock; sleep 2; shot 80-locked
  : > "$run/spied"
  for chord in XF86AudioRaiseVolume XF86MonBrightnessUp XF86AudioMute XF86AudioPlay Super+b Super+space; do press "$chord"; done
  key Super+Return 1.5; ipc state.get | jq -c '[.windows[] | .appId]'
fi

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
