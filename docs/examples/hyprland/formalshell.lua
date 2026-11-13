-- MAC APPS
-- A Mac app under atoll takes SUPER as Command, and Hyprland takes a bound
-- SUPER chord before any window sees it. While a Mac window has focus the
-- shell switches to the submap `mac`, which holds every bind of the default
-- map except the SUPER and SUPER + SHIFT chords the user has not reserved
-- (settings.json `macApps.reservedBinds`), so those reach the app.
--
-- Lua cannot list the binds with their actions, so they are recorded as the
-- config makes them: load this file before any bind that should keep
-- working over a Mac window. The names are global because the shell calls
-- them through `hyprctl eval`. A reload runs this file again in a new Lua
-- state: the globals and the `hl.on` handlers of the one before are gone
-- with its binds (dev/smoke.d/mac_faults.sh counts them).
formalshell_binds = {}
if not formalshell_raw then
  formalshell_raw = { bind = hl.bind, unbind = hl.unbind, define_submap = hl.define_submap }
  formalshell_submap_depth = 0
  hl.bind = function(chord, action, opts)
    local handle = formalshell_raw.bind(chord, action, opts)
    if formalshell_submap_depth == 0 then
      formalshell_binds[#formalshell_binds + 1] = { chord = chord, action = action, opts = opts, handle = handle }
    end
    return handle
  end
  hl.unbind = function(chord)
    for i = #formalshell_binds, 1, -1 do
      if formalshell_binds[i].chord == chord then
        table.remove(formalshell_binds, i)
      end
    end
    return formalshell_raw.unbind(chord)
  end
  hl.define_submap = function(...)
    formalshell_submap_depth = formalshell_submap_depth + 1
    local ok, err = pcall(formalshell_raw.define_submap, ...)
    formalshell_submap_depth = formalshell_submap_depth - 1
    if not ok then
      error(err, 0)
    end
  end
end

-- "SUPER + SHIFT + code:10" and "super+shift+1" are one chord: modifiers
-- upper case and sorted, the key lower case, a digit row keycode as its digit.
local function mac_chord(chord)
  local mods, key = {}, ""
  for part in tostring(chord):gmatch("[^+]+") do
    local word = part:match("^%s*(.-)%s*$")
    local upper = word:upper()
    if upper == "CONTROL" then
      upper = "CTRL"
    end
    if upper == "SUPER" or upper == "SHIFT" or upper == "CTRL" or upper == "ALT" then
      mods[#mods + 1] = upper
    else
      key = word:lower()
    end
  end
  local code = tonumber(key:match("^code:(%d+)$"))
  if code and code >= 10 and code <= 19 then
    key = tostring((code - 9) % 10)
  end
  table.sort(mods)
  return table.concat(mods, "+"), key
end

-- Hyprland knows a submap only by its binds, and enters none it has no bind
-- for: with every recorded bind a SUPER chord gone to the app, `mac` would
-- not exist and the default map, SUPER + Q closing the window, would stay.
-- This bind keeps a submap there, does nothing and passes its key on.
local function mac_anchor()
  return formalshell_raw.bind("SUPER + SHIFT + CTRL + ALT + F24", function() end, { non_consuming = true })
end

-- SUPER alone or with SHIFT on a key: the two layers Mac key equivalents use.
local function mac_handed(mods, key)
  if mods ~= "SUPER" and mods ~= "SHIFT+SUPER" then
    return false
  end
  return not (key:match("^mouse") or key:match("^switch:"))
end

-- Builds the submap from the recorded binds, replacing the one before, and
-- answers how many binds were recorded.
function formalshell_mac_keys(reserved)
  for _, handle in ipairs(formalshell_mac_binds or {}) do
    handle:remove()
  end
  formalshell_mac_binds = {}
  local keep = {}
  for _, chord in ipairs(reserved) do
    local mods, key = mac_chord(chord)
    keep[mods .. "+" .. key] = true
  end
  formalshell_raw.define_submap("mac", function()
    formalshell_mac_binds[#formalshell_mac_binds + 1] = mac_anchor()
    for _, bind in ipairs(formalshell_binds) do
      local mods, key = mac_chord(bind.chord)
      if not mac_handed(mods, key) or keep[mods .. "+" .. key] then
        formalshell_mac_binds[#formalshell_mac_binds + 1] = formalshell_raw.bind(bind.chord, bind.action, bind.opts)
      end
    end
  end)
  return #formalshell_binds
end

-- The submap while `window` is the one the shell last called a Mac window,
-- the default map over any other, leaving a submap of the user's own alone.
local function mac_follow(window)
  local over = window ~= nil and window.address == formalshell_mac_window
  local current = hl.get_current_submap()
  if over and (current == "" or current == "macapp") then
    hl.dispatch(hl.dsp.submap("mac"))
  elseif not over and (current == "mac" or current == "macapp") then
    hl.dispatch(hl.dsp.submap("reset"))
  end
end

-- A Mac app stays frontmost with its last window closed, and its key
-- equivalents still work. While no window has focus and one is, the shell
-- asks for the submap `macapp`: the `mac` submap's binds, and each of the
-- app's key equivalents, `binds` ({ chord, action }), bound to the shell,
-- which chooses that item in the app's menu. No `binds` is out of it.
function formalshell_mac_app(binds)
  for _, handle in ipairs(formalshell_mac_app_binds or {}) do
    handle:remove()
  end
  formalshell_mac_app_binds = {}
  if binds == nil then
    if hl.get_current_submap() == "macapp" then
      hl.dispatch(hl.dsp.submap("reset"))
    end
    return
  end
  local taken = {}
  for _, b in ipairs(binds) do
    local mods, key = mac_chord(b.chord)
    taken[mods .. "+" .. key] = true
  end
  formalshell_raw.define_submap("macapp", function()
    formalshell_mac_app_binds[#formalshell_mac_app_binds + 1] = mac_anchor()
    for _, bind in ipairs(formalshell_binds) do
      local mods, key = mac_chord(bind.chord)
      if not taken[mods .. "+" .. key] then
        formalshell_mac_app_binds[#formalshell_mac_app_binds + 1] = formalshell_raw.bind(bind.chord, bind.action, bind.opts)
      end
    end
    for _, b in ipairs(binds) do
      local ok, handle = pcall(formalshell_raw.bind, b.chord, b.action)
      if ok then
        formalshell_mac_app_binds[#formalshell_mac_app_binds + 1] = handle
      end
    end
  end)
  local current = hl.get_current_submap()
  if hl.get_active_window() == nil and (current == "" or current == "mac") then
    hl.dispatch(hl.dsp.submap("macapp"))
  end
end

-- The shell says whether the window it found in front, `selector`, is a Mac
-- window; no selector is no window in front. Focus may have moved since the
-- shell looked: what it says is kept for the window it looked at, and the
-- map is left as it is unless that window is still the one in front, or
-- its answer would land on another. `window.active` applies it when focus
-- comes back; the shell does not ask twice about one window.
function formalshell_mac_focus(on, selector)
  local window = hl.get_active_window()
  local asked = window
  if selector then
    asked = hl.get_window(selector)
  end
  formalshell_mac_window = on and asked and asked.address or nil
  if selector and not (window and asked and asked.address == window.address) then
    return
  end
  mac_follow(window)
end

-- A reload drops every bind, the submap's with them, and leaves the session
-- in the submap it was in: with a Mac window in front that is `mac` with no
-- bind in it and every key dead. Out of it here; a running shell builds the
-- submap and asks again after each reload.
mac_follow(nil)

-- Focus moves without the shell saying so when the shell has exited, and
-- when a window takes focus and gives it back between two of its looks: the
-- map follows the window here, so no key is left dead waiting on the shell.
hl.on("window.active", mac_follow)
-- The last window closing moves focus to none, which is no `window.active`.
hl.on("window.close", function(window)
  if window ~= nil and window.address == formalshell_mac_window then
    formalshell_mac_window = nil
    mac_follow(nil)
  end
end)

-- Window rounding, the blur behind the shell's surfaces and the frame and
-- shadow round every window follow the shell's own theme: ThemeEngine
-- publishes formalshell-chrome.lua, a table with rounding (the cards' radius),
-- blur (theme.blur) and the theme table's own window chrome (gapsIn, gapsOut,
-- borderSize, borderColor, shadow, shadowRange, shadowPower, shadowOffset,
-- shadowColor, shadowInactiveColor) and the motion curves (below), and
-- rewrites it whenever any of them changes, so the retro preset squares the corners and turns blur off,
-- metamorphosis keeps 10px and blur on, and pantheon casts elementary's own
-- shadow under a quiet 1px frame and closes the outer gap it has no screen
-- frame to leave room for. The shell never blurs a pixel itself.
--
-- dofile this from hyprland.lua. The shell runs `hyprctl reload` after each
-- publish, since a dofile'd file is not one Hyprland watches.
local config_dir = (os.getenv("XDG_CONFIG_HOME") or (os.getenv("HOME") .. "/.config")) .. "/hypr/"

-- Both files are absent until the shell's first run, and pcall is what stops
-- that from killing the rest of the config.
local function published(name, fallback)
  local ok, loaded = pcall(dofile, config_dir .. name)
  if ok and type(loaded) == "table" then
    return loaded
  end
  return fallback
end

local colors = published("formalshell-colors.lua", {
  primary = "rgb(9ecafc)",
  primaryForeground = "rgb(00325a)",
  background = "rgb(101418)",
  foreground = "rgb(e0e2e8)",
  border = "rgb(42474e)",
  destructive = "rgb(ffb4ab)",
  warning = "rgb(bdc9d3)",
})

local chrome = published("formalshell-chrome.lua", {
  rounding = 10,
  blur = true,
  gapsIn = 4,
  gapsOut = 8,
  borderSize = 1,
  borderColor = "primary",
  shadow = false,
  shadowRange = 4,
  shadowPower = 3,
  shadowOffset = { 0, 0 },
  shadowColor = "rgba(000000ed)",
  shadowInactiveColor = "rgba(000000ed)",
})

-- A chrome colour is an rgba literal or the name of a role in the colours
-- table.
local function color(c)
  return colors[c] or c
end

hl.config({
  decoration = {
    rounding = chrome.rounding,
    -- vibrancy 0 is Pond's niri `saturation 1.0`: the blur behind a card
    -- keeps the wallpaper's own saturation instead of tinting the card
    -- toward it.
    blur = {
      enabled = chrome.blur,
      size = 8,
      passes = 2,
      vibrancy = 0,
    },
    shadow = {
      enabled = chrome.shadow,
      range = chrome.shadowRange,
      render_power = chrome.shadowPower,
      offset = chrome.shadowOffset,
      color = color(chrome.shadowColor),
      color_inactive = color(chrome.shadowInactiveColor),
    },
  },

  -- borderColor is the theme table's own window frame: metamorphosis hangs
  -- the wallpaper's primary on the focused window, pantheon the quiet border
  -- on every window. gapsIn and gapsOut are the theme's own window spacing:
  -- 4 and 8 under metamorphosis, 4 and 6 under pantheon, which wears no
  -- screen frame and so has nothing to leave room for.
  general = {
    gaps_in = chrome.gapsIn,
    gaps_out = chrome.gapsOut,
    border_size = chrome.borderSize,
    col = {
      active_border = color(chrome.borderColor),
      inactive_border = colors.border,
    },
    -- The owner's choice for games, with the `immediate` rule below: tearing
    -- is allowed, and only windows that rule marks ever tear.
    allow_tearing = true,
  },

  -- The owner's choice: VRR only while a window is fullscreen. Black under
  -- everything, so the session arrives out of the greeter's black
  -- (docs/superpowers/specs/2026-10-30-seams.md).
  misc = {
    vrr = 2,
    background_color = "rgb(000000)",
    -- A shell restarted while locked takes the lock back
    -- (crates/formalshell-rs/src/wayland/lock.rs).
    allow_session_lock_restore = true,
  },

  group = {
    col = {
      border_active = colors.primary,
      border_inactive = colors.border,
      border_locked_active = colors.destructive,
    },
  },
})

-- Windows, workspaces and layers move on the shell's own two clock families
-- (DESIGN.md section 1 Motion), out of the chrome file's `motion`: anything
-- with a position or a size rides a spatial curve, which carries it a few
-- pixels past its place and back as the shell's cards do, and opacity and
-- colour an effects curve, which never overshoots. Each curve is the shell's
-- own control points and each speed its duration in tenths of a second.
-- `motion.enabled: false` turns the animations off with the shell's own.
local motion = chrome.motion or {
  enabled = true,
  spatialFast = { curve = { { 0.42, 1.67 }, { 0.21, 0.9 } }, speed = 3.5 },
  spatial = { curve = { { 0.38, 1.21 }, { 0.22, 1 } }, speed = 5 },
  spatialSlow = { curve = { { 0.39, 1.29 }, { 0.35, 0.98 } }, speed = 6.5 },
  effectsFast = { curve = { { 0.31, 0.94 }, { 0.34, 1 } }, speed = 1.5 },
  effects = { curve = { { 0.34, 0.8 }, { 0.34, 1 } }, speed = 2 },
  effectsSlow = { curve = { { 0.34, 0.88 }, { 0.34, 1 } }, speed = 3 },
}
hl.config({ animations = { enabled = motion.enabled } })
for _, family in ipairs({ "spatialFast", "spatial", "spatialSlow", "effectsFast", "effects", "effectsSlow" }) do
  hl.curve("formalshell_" .. family, { type = "bezier", points = motion[family].curve })
end
local function leaf(name, family)
  hl.animation({ leaf = name, enabled = true, bezier = "formalshell_" .. family, speed = motion[family].speed })
end
-- A window arriving, moving or resizing travels on spatial and leaves on
-- spatialFast, a workspace slides on spatial, and a fade or a border colour
-- takes its effects clock.
leaf("windows", "spatial")
leaf("windowsOut", "spatialFast")
leaf("layers", "spatial")
leaf("layersOut", "spatialFast")
leaf("fade", "effects")
leaf("fadeOut", "effectsFast")
leaf("border", "effectsSlow")
leaf("workspaces", "spatial")
leaf("specialWorkspace", "spatial")
-- A new output's zoom from 2x stays off: at boot it runs over the lock's first
-- frames, so the splash picture landed at twice the size plymouth drew it.
hl.animation({ leaf = "monitorAdded", enabled = false })

-- The pointer stays hidden until the shell has faded the desktop in. Hidden
-- twice over: Hyprland draws it for its first frames whatever
-- `cursor.invisible` says, so its cursor manager starts on a theme of
-- transparent cursors too. That reads XCURSOR_THEME once at start, so the
-- variable goes back to the real theme for everything started after it. The
-- shell then writes its marker for this instance and calls
-- formalshell_arrived, which puts the real theme on the pointer and shows it,
-- so a reload after that (the shell's own after each publish) leaves it
-- shown. A shell that never starts gets it back ten seconds in, marker and all.
local blank = "formalos-blank"
local function real_cursor_theme()
  local theme = os.getenv("XCURSOR_THEME") or "default"
  if theme == blank then
    theme = os.getenv("FORMALSHELL_XCURSOR_THEME") or "default"
  end
  return theme
end
function formalshell_arrived()
  local theme = real_cursor_theme()
  hl.env("XCURSOR_THEME", theme)
  hl.exec_cmd("hyprctl setcursor " .. theme .. " " .. (os.getenv("XCURSOR_SIZE") or "24"))
  hl.config({ cursor = { invisible = false } })
end
local arrived = (os.getenv("XDG_RUNTIME_DIR") or "") .. "/formalshell/arrived-" .. (os.getenv("HYPRLAND_INSTANCE_SIGNATURE") or "")
local marker = io.open(arrived, "r")
local arriving = not marker
if marker then
  marker:close()
end
-- Called again after any later file that sets XCURSOR_THEME itself (the
-- installer's entry does, after hypr-user.lua). Only before the start: a
-- reload after it would hand the blank theme to whatever starts next.
function formalshell_blank_cursor()
  if arriving and not os.getenv("FORMALSHELL_CURSOR_STARTED") then
    hl.env("FORMALSHELL_XCURSOR_THEME", real_cursor_theme())
    hl.env("XCURSOR_THEME", blank)
  end
end
if arriving then
  -- One transparent pixel, as an Xcursor file, under every name Hyprland asks for at start.
  local dir = (os.getenv("HOME") or "") .. "/.local/share/icons/" .. blank .. "/cursors/"
  os.execute("mkdir -p '" .. dir .. "'")
  for _, name in ipairs({ "default", "left_ptr", "arrow", "top_left_arrow", "wait", "watch", "progress", "left_ptr_watch", "text", "xterm", "pointer", "hand2" }) do
    local f = io.open(dir .. name, "wb")
    if f then
      f:write("Xcur\16\0\0\0\0\0\1\0\1\0\0\0\2\0\253\255\24\0\0\0\28\0\0\0$\0\0\0\2\0\253\255\24\0\0\0\1\0\0\0\1\0\0\0\1\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0")
      f:close()
    end
  end
  formalshell_blank_cursor()
  hl.config({ cursor = { invisible = true } })
  hl.on("hyprland.start", function()
    hl.env("FORMALSHELL_CURSOR_STARTED", "1")
    hl.env("XCURSOR_THEME", real_cursor_theme())
  end)
end
hl.on("hyprland.start", function()
  hl.exec_cmd([==[sleep 10 && mkdir -p "$XDG_RUNTIME_DIR/formalshell" && touch "$XDG_RUNTIME_DIR/formalshell/arrived-$HYPRLAND_INSTANCE_SIGNATURE" && hyprctl eval 'formalshell_arrived()']==])
end)

-- An Electron app in atoll's Mac mode draws no frame of its own and cannot
-- carry atoll's xdg tag; the shell tags its windows `atoll` once its process
-- registers its menu. A Mac window has no border and the Mac's continuous
-- corner: 24 px along each edge at power 3, measured off a macOS 27 capture
-- of a frameless Electron panel (atoll's fixtures/capture.sh electron-panel).
hl.window_rule({
  name = "formalshell-mac-electron",
  match = { tag = "atoll" },
  border_size = 0,
  rounding = 24,
  rounding_power = 3,
})

-- Steam starts each Proton game under XWayland with a steam_app_<id> class.
hl.window_rule({
  name = "games-immediate",
  match = { class = "^steam_app_.*$" },
  immediate = true,
})

-- ignore_alpha leaves anything at or below that opacity unblurred, which is
-- what keeps the bar strip's empty band between the cells clear.
--
-- The three modal namespaces below take 0.6 instead of 0.2: their layers cover
-- the whole output and carry a 0.5 black scrim, which falls under that mark and
-- so only darkens the desktop, while the 0.85 card stays over it and keeps its
-- blur. A `theme.surfaceOpacity` under 0.6 puts the card under the mark too and
-- loses the blur on those three (docs/USAGE.md).
--
-- The panel and notification namespaces take 0.6 too: their cards hang a
-- five-layer cast whose sum passes 0.2 just outside the card, so at 0.2
-- Hyprland would frost a halo round every panel and toast. The cast stays
-- under 0.6 and the 0.85 card over it.
--
-- no_anim on every one of them: the shell animates its own surfaces (DESIGN.md
-- section 1 Motion), so a compositor animation on the layer runs a second one
-- over the top of it.
hl.layer_rule({
  name = "formalshell-bar",
  match = { namespace = "formalshell:bar" },
  blur = chrome.blur,
  ignore_alpha = 0.2,
  no_anim = true,
})

hl.layer_rule({
  name = "formalshell-panel",
  match = { namespace = "formalshell:panel" },
  blur = chrome.blur,
  ignore_alpha = 0.6,
  no_anim = true,
})

hl.layer_rule({
  name = "formalshell-menu",
  match = { namespace = "formalshell:menu" },
  blur = chrome.blur,
  ignore_alpha = 0.6,
  no_anim = true,
})

hl.layer_rule({
  name = "formalshell-radio",
  match = { namespace = "formalshell:radio" },
  blur = chrome.blur,
  ignore_alpha = 0.6,
  no_anim = true,
})

-- The OSD pill buds off the bottom line the way a panel buds off the bar's,
-- so it is drawn at the same surfaceOpacity the line it grows out of is: one
-- silhouette across two windows cannot be opaque on one side of the seam.
-- The arrival and departure veil fades itself.
hl.layer_rule({
  name = "formalshell-veil",
  match = { namespace = "formalshell:veil" },
  no_anim = true,
})

hl.layer_rule({
  name = "formalshell-osd",
  match = { namespace = "formalshell:osd" },
  blur = chrome.blur,
  ignore_alpha = 0.2,
  no_anim = true,
})

-- The window switcher's card is drawn at Gala's own 0.6 over the desktop, and
-- the rest of its layer is fully transparent, so the 0.2 mark keeps the blur
-- under the card and nowhere else. No scrim: the switcher dims nothing.
hl.layer_rule({
  name = "formalshell-switcher",
  match = { namespace = "formalshell:switcher" },
  blur = chrome.blur,
  ignore_alpha = 0.2,
  no_anim = true,
})

hl.layer_rule({
  name = "formalshell-tooltip",
  match = { namespace = "formalshell:tooltip" },
  blur = chrome.blur,
  ignore_alpha = 0.2,
  no_anim = true,
})

-- The polkit consent card and a plugin's overlay are drawn at the same
-- surfaceOpacity every other card is, so they take the same blur, and their
-- scrims fall under the 0.6 mark above and darken the desktop instead.
hl.layer_rule({
  name = "formalshell-polkit",
  match = { namespace = "formalshell:polkit" },
  blur = chrome.blur,
  ignore_alpha = 0.6,
  no_anim = true,
})

hl.layer_rule({
  name = "formalshell-plugin-overlay",
  match = { namespace = "formalshell:plugin-overlay" },
  blur = chrome.blur,
  ignore_alpha = 0.6,
  no_anim = true,
})

-- The toast cards are drawn at surfaceOpacity too; the layer between and
-- around them carries nothing but their casts, which the 0.6 mark above
-- leaves unblurred.
hl.layer_rule({
  name = "formalshell-notifications",
  match = { namespace = "formalshell:notifications" },
  blur = chrome.blur,
  ignore_alpha = 0.6,
  no_anim = true,
})

-- docs/USAGE.md spells the same invocation out for every other target.
local fs_call = "formalshell-ipc call "

-- A Lua bind shows up in `hyprctl binds` as an anonymous function, so the
-- description is all the launcher's keybinds route has to print for it.
local function bind(chord, action, description, opts)
  opts = opts or {}
  opts.description = description
  hl.bind(chord, action, opts)
end

local function fs_bind(chord, args, opts)
  bind(chord, hl.dsp.exec_cmd(fs_call .. args), args, opts)
end

-- UTILITIES
fs_bind("SUPER + Space", "menu toggle")
fs_bind("SUPER + ALT + Space", "menu summon apps")
fs_bind("SUPER + CTRL + E", "menu summon emoji")
fs_bind("SUPER + CTRL + C", "menu summon capture")
fs_bind("SUPER + CTRL + O", "menu summon toggles")
fs_bind("SUPER + CTRL + S", "menu summon share")
fs_bind("SUPER + CTRL + R", "menu summon reminder")
fs_bind("SUPER + Escape", "menu summon system")
fs_bind("SUPER + K", "menu summon keybinds")
fs_bind("SUPER + ALT + M", "menu summon appmenu")
fs_bind("SUPER + O", "workspaces overview toggle")
fs_bind("SUPER + Z", "zen toggle")
fs_bind("SUPER + CTRL + Q", "menu summon calc")
fs_bind("SUPER + CTRL + M", "mirror toggle")

-- The wallpaper route is the picker grid: pick one and the whole palette
-- follows it. There is no "advance to the next wallpaper" IPC verb.
fs_bind("SUPER + CTRL + Space", "menu summon wallpaper")
fs_bind("SUPER + SHIFT + CTRL + Space", "menu summon theme")

-- Settings and Information open centred in the launcher card, as Pond's
-- settings window does. SUPER + comma is the notifications' (Omarchy).
fs_bind("SUPER + ALT + S", "panel toggle settings")
fs_bind("SUPER + ALT + I", "panel toggle info")

-- Quick settings, on GNOME's own chord. Opened from a key it hangs at the
-- end of the bar's line rather than under the chevron.
fs_bind("SUPER + S", "panel toggle quicksettings")

-- Panels by name. `display` is the brightness and output panel, not a bar
-- cell of its own.
fs_bind("SUPER + CTRL + A", "panel toggle audio")
fs_bind("SUPER + CTRL + B", "panel toggle bluetooth")
fs_bind("SUPER + CTRL + W", "panel toggle network")
fs_bind("SUPER + CTRL + P", "panel toggle power")
fs_bind("SUPER + CTRL + D", "panel toggle display")
fs_bind("SUPER + CTRL + ALT + D", "panel toggle calendar")

-- `panel toggleAt n` opens the nth panel-bearing cell of the bar's right
-- region, counted from the screen centre outward: with the default layout
-- that is power, audio, network, bluetooth, weather. Cells that open no
-- panel (tray, bell, indicators) are skipped, and a cell hidden behind a
-- collapsed chevron still holds its number.
for n = 1, 9 do
  fs_bind("SUPER + CTRL + " .. n, "panel toggleAt " .. n)
end

fs_bind("SUPER + comma", "notifications dismissOne")
fs_bind("SUPER + SHIFT + comma", "notifications dismissAll")
fs_bind("SUPER + ALT + comma", "notifications invokeLast")
fs_bind("SUPER + SHIFT + ALT + comma", "notifications showHistory")
fs_bind("SUPER + CTRL + comma", "notifications toggleDnd")

-- Caffeinate is the idle inhibitor: on means the screensaver and any
-- ext-idle-notify daemon (swayidle, hypridle) never fire.
fs_bind("SUPER + CTRL + I", "caffeinate toggle")
fs_bind("SUPER + CTRL + N", "nightlight toggle")
fs_bind("SUPER + CTRL + SHIFT + N", "overnight toggle")
-- Collapses the bar's chevron group rather than the whole strip; the shell
-- has no verb for hiding the bar itself.
fs_bind("SUPER + SHIFT + Space", "bar chevron toggle")
fs_bind("SUPER + CTRL + L", "lock lock")

-- The window switcher (off with `switcher.enabled: false` in settings.json,
-- which answers these with an error string). Alt+Tab opens the
-- card with the cursor on the window you came from and walks the row from
-- there, and the commit is bound to the RELEASE of the modifier, which is
-- what makes a held Alt feel like Alt+Tab anywhere else. A release bind takes
-- the modifier as a keysym rather than as a modifier (`Alt_L`), so the bind
-- fires on every Alt release, switcher open or not; `switcher commit` with
-- nothing open is a no-op that says so.
--
-- `transparent` matters. Hyprland shadows a bind whose key is held down while
-- another bind fires (`CKeybindManager::shadowKeybinds`), and nothing lifts
-- that until the key is up, which is one event too late: holding Alt and
-- pressing Tab shadows the Alt release bind, so a plain release bind fires on
-- a bare Alt tap and never after an Alt+Tab, which is the one time it is
-- wanted (owner, 2026-09-18). A transparent bind is the only kind
-- `shadowKeybinds` skips. The mods stay on: Hyprland emits the key event
-- before it updates its xkb state (`devices/Keyboard.cpp`), so ALT is still
-- held as far as the bind table is concerned when Alt_L comes up.
--
-- Alt_R is bound beside it for a right-hand Alt; on a layout where that key
-- is AltGr it carries a different modifier and simply never matches. Escape
-- and Enter still cancel and commit from the card's own keyboard.
--
-- `repeating` on the two Tab binds: Hyprland repeats a bind only when asked,
-- so without it a held Tab steps once and stops.
fs_bind("ALT + Tab", "switcher next", { repeating = true })
fs_bind("ALT + SHIFT + Tab", "switcher prev", { repeating = true })
fs_bind("ALT + Alt_L", "switcher commit", { release = true, transparent = true })
fs_bind("ALT + Alt_R", "switcher commit", { release = true, transparent = true })

-- `screenshot pick` is the route with the toolbar, keyboard window selection
-- and the hand-off to recording. `screenshot region` is the plain slurp path
-- and looks identical from the outside, which is how it gets bound here by
-- mistake (ScreenshotIpc.qml's header).
fs_bind("Print", "screenshot pick smart default")
fs_bind("SUPER + CTRL + Print", "capture text")
fs_bind("ALT + Print", "record toggle screen none")

-- MEDIA
-- The hardware key changes the value and the shell is told to show it: the
-- OSD reads brightness on demand and has no signal of its own to hook, and
-- volume is shown here for the same reason even though a sink change would
-- also surface it. `locked` keeps these live while the screen is locked,
-- `repeating` repeats them on hold.
local function then_osd(cmd, target)
  return hl.dsp.exec_cmd(cmd .. " && " .. fs_call .. "osd " .. target)
end

bind("XF86AudioRaiseVolume", then_osd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+", "volume"), "volume up", { locked = true, repeating = true })
bind("XF86AudioLowerVolume", then_osd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-", "volume"), "volume down", { locked = true, repeating = true })
bind("XF86AudioMute", then_osd("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle", "volume"), "mute", { locked = true })
bind("XF86AudioMicMute", hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle"), "mic mute", { locked = true })
bind("XF86MonBrightnessUp", then_osd("brightnessctl -q set 5%+", "brightness"), "brightness up", { locked = true, repeating = true })
bind("XF86MonBrightnessDown", then_osd("brightnessctl -q set 5%-", "brightness"), "brightness down", { locked = true, repeating = true })
fs_bind("XF86AudioPlay", "media playPause", { locked = true })
fs_bind("XF86AudioPause", "media playPause", { locked = true })
fs_bind("XF86AudioNext", "media next", { locked = true })
fs_bind("XF86AudioPrev", "media previous", { locked = true })

-- CLIPBOARD
fs_bind("SUPER + CTRL + V", "menu summon clipboard")
