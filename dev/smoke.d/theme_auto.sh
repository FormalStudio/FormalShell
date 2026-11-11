# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --theme-auto pins `theme.mode` to `auto` in the settings fixture and reads
# the dark schedule back out of `theme status`.
#
# The base fixture already pins location.latitude/longitude (52.52, 13.41),
# the one source of truth LocationService reads, so the schedule has to
# resolve off it: `schedule.source` is `location`, never the 20:00 to 06:00
# fallback, and it carries a real sunrise/sunset pair. The run's own clock is
# never faked: this leg recomputes dark-from-sunset-to-sunrise from the pair
# the shell itself reported plus the wall clock at the moment it reported it,
# and that is what `effective` and the live mode have to agree with. A
# sample landing within two minutes of either boundary accepts both, since
# the shell's own read and this one are seconds apart.
#
# Then one `theme mode toggle`: under `auto` a flip is a snooze of one
# cycle, so the mode flips, `override` reports it, state.json carries it
# (the shell never writes settings.json), and the override expires on a
# boundary rather than at some arbitrary time.
leg_theme_auto_flag="--theme-auto"
leg_theme_auto_order=221
leg_theme_auto_needs="jq"
leg_theme_auto_claims="theme.mode=shared"

theme_auto_before_png="$shot_dir/theme-auto-before.png"
theme_auto_after_png="$shot_dir/theme-auto-after.png"
theme_auto_status1_path="$shot_dir/theme-auto-status-1.json"
theme_auto_status2_path="$shot_dir/theme-auto-status-2.json"
theme_auto_clock_path="$shot_dir/theme-auto-clock.txt"
theme_auto_toggle_path="$shot_dir/theme-auto-toggle.txt"
theme_auto_state_path="$shot_dir/theme-auto-state.json"
theme_auto_state_json="$iso_home/.local/state/formalshell/state.json"

leg_theme_auto_fixture() {
  settings_pin '.theme.mode = "auto"'
}

leg_theme_auto_timing() {
  leg_timing 45
}

leg_theme_auto_drive() {
  local script="$shot_dir/theme-auto-drive.sh"
  write_script "$script" <<DRIVE
#!/usr/bin/env bash
date +%H:%M > "$theme_auto_clock_path"
$ipc call theme status > "$theme_auto_status1_path" 2>&1
"$grim_bin" "$theme_auto_before_png" > /dev/null 2>&1
$ipc call theme mode toggle > "$theme_auto_toggle_path" 2>&1
before=\$("$jq_bin" -r .mode "$theme_auto_status1_path")
until_jq 60 "$theme_auto_status2_path" ".mode != \"\$before\" and .override != null and .palette.mode == .mode and .palette.wallpaper == .desktop" call theme status
until_settled
$ipc call theme status > "$theme_auto_status2_path" 2>&1
cat "$theme_auto_state_json" > "$theme_auto_state_path" 2>&1
"$grim_bin" "$theme_auto_after_png" > /dev/null 2>&1
DRIVE
  wm_exec_once "bash $script"
}

# "HH:MM" to minutes since midnight, base 10 so an 08:xx hour is not read as
# a bad octal literal.
theme_auto_minutes() {
  local hm="$1"
  echo $((10#${hm%%:*} * 60 + 10#${hm##*:}))
}

leg_theme_auto_assert() {
  local mode1 effective1 key source polar sunrise sunset now_hm
  local now_min rise_min set_min want flipped mode2 effective2 override_mode until_ms until_hm

  if [ ! -s "$theme_auto_status1_path" ] || [ ! -s "$theme_auto_clock_path" ]; then
    [ -f "$theme_auto_status1_path" ] && cat "$theme_auto_status1_path" >&2
    fail "theme status under theme.mode auto produced nothing"
  fi
  cat "$theme_auto_status1_path"; echo

  key=$("$jq_bin" -r '.modeKey' "$theme_auto_status1_path")
  mode1=$("$jq_bin" -r '.mode' "$theme_auto_status1_path")
  effective1=$("$jq_bin" -r '.effective' "$theme_auto_status1_path")
  source=$("$jq_bin" -r '.schedule.source' "$theme_auto_status1_path")
  polar=$("$jq_bin" -r '.schedule.polar' "$theme_auto_status1_path")
  sunrise=$("$jq_bin" -r '.schedule.sunrise' "$theme_auto_status1_path")
  sunset=$("$jq_bin" -r '.schedule.sunset' "$theme_auto_status1_path")
  now_hm=$(cat "$theme_auto_clock_path")

  [ "$key" = "auto" ] || fail "theme status reported modeKey $key, not auto"
  # The fixture's own coordinates are what makes this `location`: a
  # `fallback` here means the schedule never saw LocationService at all.
  [ "$source" = "location" ] || fail "schedule.source is $source, not location (the fixture pins location.latitude/longitude)"
  [ "$polar" = "" ] || fail "schedule.polar is $polar at 52.52N, which has a sunrise every day"
  case "$sunrise" in [0-9][0-9]:[0-9][0-9]) ;; *) fail "schedule.sunrise is not a wall clock: $sunrise" ;; esac
  case "$sunset" in [0-9][0-9]:[0-9][0-9]) ;; *) fail "schedule.sunset is not a wall clock: $sunset" ;; esac
  echo "theme-auto: sunrise $sunrise, sunset $sunset, sampled at $now_hm"

  now_min=$(theme_auto_minutes "$now_hm")
  rise_min=$(theme_auto_minutes "$sunrise")
  set_min=$(theme_auto_minutes "$sunset")
  if [ "$now_min" -lt "$rise_min" ] || [ "$now_min" -ge "$set_min" ]; then want="dark"; else want="light"; fi
  if [ $((now_min > rise_min ? now_min - rise_min : rise_min - now_min)) -le 2 ] \
    || [ $((now_min > set_min ? now_min - set_min : set_min - now_min)) -le 2 ]; then
    echo "theme-auto: sampled within two minutes of a boundary, both modes accepted"
  else
    [ "$effective1" = "$want" ] || fail "schedule says $want at $now_hm ($sunrise/$sunset) but effective is $effective1"
    [ "$mode1" = "$want" ] || fail "schedule says $want at $now_hm ($sunrise/$sunset) but the live mode is $mode1"
  fi

  if [ "$mode1" = "dark" ]; then flipped="light"; else flipped="dark"; fi
  if [ ! -s "$theme_auto_status2_path" ]; then
    fail "theme status after the toggle produced nothing"
  fi
  cat "$theme_auto_status2_path"; echo

  mode2=$("$jq_bin" -r '.mode' "$theme_auto_status2_path")
  effective2=$("$jq_bin" -r '.effective' "$theme_auto_status2_path")
  override_mode=$("$jq_bin" -r '.override.mode' "$theme_auto_status2_path")
  until_ms=$("$jq_bin" -r '.override.untilMs' "$theme_auto_status2_path")

  [ "$mode2" = "$flipped" ] || fail "toggle under auto left the mode at $mode2, expected $flipped"
  [ "$effective2" = "$flipped" ] || fail "toggle under auto left effective at $effective2, expected $flipped"
  [ "$override_mode" = "$flipped" ] || fail "theme status reported override $override_mode, expected $flipped"
  case "$until_ms" in ''|*[!0-9]*) fail "override.untilMs is not an epoch: $until_ms" ;; esac

  # The snooze runs to a scheduled change and no further: tomorrow's sunrise
  # is minutes off today's, so either boundary within four minutes is it.
  until_hm=$(date -d "@$((until_ms / 1000))" +%H:%M)
  local until_min diff_rise diff_set
  until_min=$(theme_auto_minutes "$until_hm")
  diff_rise=$((until_min > rise_min ? until_min - rise_min : rise_min - until_min))
  diff_set=$((until_min > set_min ? until_min - set_min : set_min - until_min))
  if [ "$diff_rise" -gt 4 ] && [ "$diff_set" -gt 4 ]; then
    fail "the snooze expires at $until_hm, neither sunrise ($sunrise) nor sunset ($sunset)"
  fi
  echo "theme-auto: snooze to $flipped expires $until_hm"

  # settings.json is read-only to the shell, so the snooze can only live in
  # state.json.
  if [ ! -s "$theme_auto_state_path" ] \
    || [ "$("$jq_bin" -r '.modeOverride.mode' "$theme_auto_state_path")" != "$flipped" ]; then
    [ -f "$theme_auto_state_path" ] && cat "$theme_auto_state_path" >&2
    fail "state.json does not carry the snooze"
  fi

  if [ ! -s "$theme_auto_before_png" ] || [ ! -s "$theme_auto_after_png" ]; then
    fail "missing theme-auto screenshot pair ($theme_auto_before_png / $theme_auto_after_png)"
  fi
  if cmp -s "$theme_auto_before_png" "$theme_auto_after_png"; then
    fail "theme-auto before and after frames are byte-identical: the snooze recoloured nothing"
  fi
  echo "SMOKE_THEME_AUTO_BEFORE $theme_auto_before_png"
  echo "SMOKE_THEME_AUTO_AFTER $theme_auto_after_png"
}
