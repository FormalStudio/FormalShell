# What is verified

The spec is `docs/superpowers/specs/2026-11-11-shoji.md`, sections 2 to 7. The
config it replaces is `hyprland.lua` with the g815's `host.lua`. Every line
below is one behaviour and how it was checked:

- **rig**: run on the rig, guest `fstest3@fstestmac`, one vkms output of
  1024x768 on software GL. The script is in `wm/dev/`; `dev/check.sh <script>`
  runs it again and brings its screenshots to `wm/target/rig/`. A name like
  `11-overview` is the screenshot that shows it.
- **headless**: `formalwm/src/headless.rs`, the engine on the real core with
  two outputs and no display (`dev/rig.sh test -p formalwm`).
- **unit**: `cargo test` in `wm/`, by test name.
- **no**: not verified, with the reason.

The rig has no touchpad and no other libinput device (keys come from the
compositor's `--key` and from wtype's virtual keyboard, the pointer from a
virtual pointer), one output, no lid, no Xwayland, no VRR and no HDR. What
needs one of those is headless, unit or **no** below.

Sessions and what they need: `session.sh`, `session2.sh`, `tape.sh`,
`rules.sh`, `layers.sh`, `drags.sh`, `overview.sh` run bare. `host.sh` with
`FWM_HOST=host-default.json`, `lid.sh` with `FWM_HOST=host-lid.json`, `misc.sh`
with `FWM_HOST=host-misc.json`, `motion.sh` with `FWM_TIMING=1`, `shell.sh`
with `FWM_SHELL` naming FormalShell, `binds.sh` with `FWM_KEYS=wtype` and
`FWM_SHELL`.

## Spec 2: the engine

| Behaviour | Check | Evidence |
| --- | --- | --- |
| `formalwm` is the compositor, on `shojiwm_rs`, in its own workspace | rig | every session starts it on the tty |
| Settings are data in `~/.config/formalwm/`, read at start | rig | `session.sh` start: the nine names, 73 binds; `host.sh`: the host file's pins and mode |
| Read again on the control socket's `reload` | rig | `session.sh`, `host.sh`: `config.reloaded`, new binds and outputs in force |
| A bad file keeps the running settings and reports the error | rig | `session.sh`: `config.failed` with `binds[0]: unknown action`, windows unmoved, SUPER+Return still opens a terminal |
| The shell's settings format: comments, trailing commas | unit | `comments_and_trailing_commas_are_allowed`, `the_shared_file_loads_on_its_own` |
| The host file lays over the shared one | rig, unit | `host.sh`; `the_host_file_lays_over_the_shared_one`, `g815_lays_its_outputs_over_the_shared_file` |
| Wire types are `crates/fs-wm-proto` | unit | its eight tests read and write the spec's JSON |
| Columns of stacked windows | rig | `tape.sh`: `[8,8 500x372]` over `[8,388 500x372]`, `05-stack` |
| Default column width 0.5 | rig | `tape.sh`: 500 of 1024 less the gaps |
| Presets 0.333, 0.5, 0.667, 1.0 | rig, unit | `tape.sh`: SUPER+R to 670, SUPER+F to 1008; the wrap to 0.333 in `presets_cycle_upward_and_wrap` |
| A lone column keeps its width and is centred | rig | `tape.sh`: `[262,8 500x752]` |
| Focus fits the column by the least scroll, no recentring | rig, unit | `tape.sh`: focus three moves nothing, focus two scrolls one column; `focus_fits_by_the_least_scroll_and_does_not_recentre` |
| 8 px between everything, from the theme | rig, unit | `tape.sh`: x 8, 516; `session.sh` with `gapsIn` 8 and `gapsOut` 16: `[16,16 488x736]`, `[520,16 488x736]`; `eight_pixels_between_everything` |
| Nine workspaces with the spec's names | rig | `session.sh` start |
| Stacked vertically, a switch slides vertically | rig | `motion.sh`: `one y` frame by frame, x unchanged; `08-switching` |
| A workspace pinned to an output by the host settings | rig, headless, unit | `host.sh` on the one output (2, 3, 4 pinned, 3 in view at the start); headless: stacks `1 2 3 5 6 7 9` and `4 8`; `workspaces_pinned_by_the_host_live_on_their_output` |
| Focus changes on click only | rig | `rules.sh`: the pointer over another window leaves the focus; `session2.sh`: a click moves it |
| The hovered window still gets pointer events | rig | `rules.sh`: the unfocused client prints `hover` |
| Every bind in `hyprland.lua`, same chords | rig | the bind table below |
| The es number row by keycode | rig | `tape.sh` through `--key` and again through wtype typing the es digits; SUPER+SHIFT+4 too |
| Three fingers sideways drag the tape 1:1 | headless | scroll 716 for 300 px of fingers from 1016 |
| A release flings and snaps to a column | headless, unit | rests on a column edge with the focus in view; `a_released_scroll_snaps_a_column_edge_to_a_view_edge` |
| Three fingers up or down walk workspaces, past the neighbour | headless, unit | held at half a workspace, a flick lands two away; `a_held_stack_follows_the_fingers_past_the_neighbour` |
| Four fingers up open the overview | headless | progress 0.5 under the fingers, open on release |
| The same on a real touchpad | no | the rig has none |
| Rule matchers: app id, title, floating | rig | `rules.sh` |
| Rule matchers: xwayland, toplevel tag | unit | `tag_and_xwayland_matchers`; no Xwayland on the rig, and the core reports no tag |
| Rule effects: workspace, float, pin, size, position | rig | `rules.sh`, the rule table below |
| Rule effect: opacity | no | no rule of the owner's sets one; it takes the path of the host's opacity, which is on the rig (`host.sh`, `misc.sh`) |
| Motion on the six families | rig, unit | the motion section below |
| Blur and alpha threshold per layer namespace | rig | `layers.sh` |
| No compositor animation on a `formalshell:*` layer | no | nothing to observe: the engine animates no layer and the core has no layer animation. `animate` is read (`a_layer_rule_by_namespace`) |
| A settings reload keeps every window where it is | rig, unit | `session.sh`: same workspaces and columns after `reload`; `a_settings_reload_keeps_every_window_where_it_is` |
| Session start runs `formalshell-start.sh` once the socket is up | rig | `session2.sh`: the script wrote `display wayland-1, socket up` |
| `shojiwm_rs` and `shojiwm_lib` by git rev | no | path dependencies until the last step of this work |

## Spec 3: the socket

| Behaviour | Check | Evidence |
| --- | --- | --- |
| The JSON socket at `$XDG_RUNTIME_DIR/shojiwm-$WAYLAND_DISPLAY.sock`, newline JSON | rig | every `ipc` call of every session |
| Request, answer, error, broadcast envelopes | rig, unit | `session2.sh`: `no such window: no-such-window`; `the_envelope_is_shojiwms` |
| `$SHOJIWM_SOCKET` is never bound by the engine | rig, unit | `reload` written to `$SHOJIWM_SOCKET` works in `session.sh`, `host.sh`, `binds.sh`; `the_json_socket_is_not_the_control_socket` |
| `state.changed` once per tick, dropped when unchanged | rig, unit | `session.sh`: one broadcast across one focus change, none in the idle seconds around it; `the_same_model_publishes_the_same_json` |
| `State.outputs`, logical | rig | `session2.sh`: 512x384 at scale 2 |
| `State.workspaces`: id, idx, name, output, active, focused | rig | `session.sh` start |
| `workspaces[].urgent`, `columns` | unit | `zen_fullscreen_urgency_and_the_shown_overlay`, `the_state_the_socket_publishes` |
| `workspaces[].zen` | rig | `tape.sh`, `session.sh` |
| `windows[].id` is the foreign-toplevel identifier | rig | the backend agent's switcher captures are keyed by it on this build |
| `title`, `appId` | rig | `rules.sh` prints both for every app |
| `initialAppId`, `initialTitle` | unit | `the_state_the_socket_publishes` |
| `pid` | rig | `shell.sh`: each pid's `/proc/<pid>/comm` is `foot` |
| `workspaceId` empty on an overlay, `overlay`, `shown` | rig | `session.sh`: the console parked, shown, parked |
| `focused`, `floating`, `fullscreen` | rig | `session.sh`, `session2.sh` |
| `windows[].urgent` | unit | `zen_fullscreen_urgency_and_the_shown_overlay` |
| `column`, `row`, -1 when floating | rig | `tape.sh`, `session2.sh` |
| `rect` is the layout's, not a frame of the animation | rig | `motion.sh` reads the frames from the engine's frame log while `state.get` already has the target |
| `focusedWindowId`, `focusedWorkspaceId`, `focusedOutput`, `overview` | rig | `session.sh` |
| `keyboard` | rig | `host.sh`: `es`, then `us` after a reload |
| `cursorHidden` | rig | `rules.sh`, `session2.sh` |
| `chrome` | rig | `session.sh`: `gapsOut` 16, `activeBorder` `#ff0000`; `shell.sh`: the shell's own theme |
| `workspace.focus` | rig | `session.sh` |
| `workspace.focusAt` | rig, unit | `session2.sh` by idx; with `output` in `workspaces_by_stack_position_and_by_neighbour` |
| `window.focus` | rig | `motion.sh`, `drags.sh` |
| `window.close` | rig | `session2.sh` |
| `window.float`, set and toggle | rig | `session2.sh` |
| `window.fullscreen` | rig | `session2.sh`: `[0,0 1024x768]` and back |
| `window.moveTo`, focus stays | rig, unit | `session.sh`; `a_window_sent_away_leaves_the_focus_here_unless_followed` |
| `window.swap` | rig | `session2.sh` |
| `window.moveColumn` | rig, unit | `session2.sh` to the end; `before` in `move_column_before_another_or_to_the_end` |
| `window.place` | rig | `session.sh`: the console at `[112,40 800x420]` |
| `overlay.park`, `overlay.show` | rig | `session.sh`, `motion.sh` |
| `zen.set` | rig | `tape.sh` by id, on and off |
| `overview.set` | rig | `motion.sh`, `overview.sh` |
| `spawn` with `cwd` | rig | `session2.sh` |
| `bind.escape` | rig | `session2.sh`: runs once while set, not after; `misc.sh` |
| `binds.list` | rig | `session.sh`: 73 rows, `mods` in the wire order, `code:10` |
| `cursor.pos` | rig | `session2.sh`, `misc.sh` |
| `cursor.hide` | rig | `session2.sh` |
| `layers.list` | rig | `session2.sh`: three layers bottom first with boxes and pids; `shell.sh`: the shell's own |
| `outputs.get` | rig | `session2.sh`: 34 modes, 60.004 Hz; `lid.sh`: zeros when disabled |
| `outputs.set` `scale` | rig | `session2.sh`: 2 and back, `30-scale2` |
| `outputs.set` `enabled` | rig | `overview.sh`: off, on, every window where it was |
| `outputs.set` `mirror` | no | one output on the rig |
| `outputs.set` `color` | no | vkms has no HDR; the core is given `hdr` on a `cm` other than `srgb`, `bitdepth` and the SDR values have no core setting |
| `outputs.create`, `outputs.remove` | rig | `session2.sh`: both answer an error, the core had no such output when this was run |
| `theme.set` | rig | `session.sh`, `motion.sh`, `misc.sh`, `layers.sh` |
| The last theme stands across a settings reload | rig | `overview.sh` |
| `session.exit` | rig | `session2.sh`: socket gone, no `formalwm` left |
| `config.reloaded`, `config.failed` | rig | `session.sh` |
| `Output.vrr` | no | always false: the core has no VRR |
| `Output` and `Theme` as the spec writes them | unit | `an_output_row_and_its_colour_read_as_the_spec_writes_them`, `a_partial_theme_keeps_the_defaults_for_the_rest`, `the_default_chrome_is_what_chrome_rs_writes` |

## Spec 4: overview

| Behaviour | Check | Evidence |
| --- | --- | --- |
| The output zooms out to its workspaces stacked, each its whole tape, live windows | rig | `session.sh`: `11-overview`, `12-overview-down` |
| The shell's layers stay where they are | rig | `shell.sh`: `41-shell-overview` |
| Opens on the bind | rig | `session.sh`: SUPER+O; `overview.sh`: SUPER+E |
| Opens on `overview.set` | rig | `motion.sh`, `overview.sh` |
| Opens on four fingers up, closes on four down | headless | |
| Opens from the shell's hot corner | no | the shell's call; it is `overview.set` |
| Escape closes | rig | `session2.sh` |
| A click on empty space closes | rig | `overview.sh` |
| A click on a window focuses it and closes | rig | `session.sh`: `13-after-click` |
| A click on a workspace goes there | rig | `overview.sh` |
| A window dragged between columns | rig | `overview.sh` |
| A window dragged between workspaces | rig | `session2.sh`: onto the next workspace's panel |
| A window dropped between two workspaces opens one there | rig | `session2.sh`: `d1` between 1 and 2, `28-new-workspace` |
| Two-finger scroll walks workspaces | rig | `session2.sh`, the wheel of a virtual pointer |
| The arrow keys walk | rig | `session.sh` Down and Up; `overview.sh` Left and Right |
| Typing does nothing | rig | `overview.sh`: letters and a digit leave it open and unchanged |
| The zoom rides the spatial clock | rig | `motion.sh`: progress 0 to 101 to 100 |
| A gesture drives it 1:1 | headless | |
| The shell's own overview never opens | no | the shell's side |

## Spec 5: zen

| Behaviour | Check | Evidence |
| --- | --- | --- |
| A layout mode, not a float: `floating` stays false, the column is kept | rig | `tape.sh`: `three c2` throughout |
| The zen window takes the zen size, centred | rig | `tape.sh`: `[12,46 1000x676]`, `10-zen` |
| Every other column slides off the side it was on | rig | `tape.sh`: -1524, -1016 to the left, 2048 to the right |
| The tape does not scroll while it holds | unit | `zen_takes_the_centre_and_the_tape_slides_off_its_sides` |
| Toggling again slides the tape back, the window in its column at its width | rig | `tape.sh`: `the tape is as it was` |
| Closing the window brings the tape back | rig | `tape.sh` |
| A floating window in zen returns to its float rect | rig | `tape.sh`: `[256,154 512x461]` again |
| `workspaces[].zen` | rig | `tape.sh` |

## Spec 6: overlays

| Behaviour | Check | Evidence |
| --- | --- | --- |
| The rule for `dev.formalshell.console` maps it on `formalshell-console`, floating, hidden | rig | `session.sh`: `ov` set, `ws` empty, `shown` false, no frame of it on the workspace |
| `overlay.show` brings it onto the focused output and focuses it | rig | `session.sh`: `14-console` |
| The workspace under it dimmed 0.6 | rig | `motion.sh`: a window under it goes from 42 74 106 to 17 30 42, `92-dimmed` |
| `overlay.park` hides it and the focus goes back | rig | `session.sh`: `15-parked` |
| It belongs to no output's stack | rig, unit | `session.sh`: it is in no workspace of `State`; `the_console_maps_parked_and_shows_over_the_focused_output` |

## Spec 7: drag and drop

| Behaviour | Check | Evidence |
| --- | --- | --- |
| The drag view with the client's icon | rig | `drags.sh`: `71-card-at-home` |
| The action badge | rig | `drags.sh`: a cross over its own window, a plus over the terminal, `72-card-over-terminal` |
| The fly-back on a refused drop | rig | `drags.sh`: `75-flying` has the card between the hand and where the drag began |
| The shell's look | rig | `drags.sh`: the theme's surface, border and primary |
| Held at the corner, the overview opens (data) | rig | `drags.sh`: `78-corner` |
| Held at the corner, the overview opens (a window) | rig | `session2.sh`: `25-sprung` |
| Held over a workspace there, the session goes to it | rig | `drags.sh`, `session2.sh` |
| Held at a tape edge, the tape scrolls | rig | `drags.sh`: scroll to 1016 and back, with data and with a window in the hand |
| Held over a window, that window is focused and the drop lands in it | rig | `drags.sh`: the text arrives in the terminal and the keyboard follows, `73-dropped` |
| SUPER and the left button lift a window out of the tape | rig | `session2.sh`: `21-dragging` |
| Dropped into the column gap under the pointer | rig, unit | `session2.sh`: `22-dropped`; `a_drop_lands_in_the_gap_under_the_pointer` |
| Dropped onto a workspace in the overview | rig | `session2.sh`: `26-dropped-in-overview` |

## hyprland.lua, top to bottom

| Lines | What | Check | Evidence |
| --- | --- | --- | --- |
| 1-8 | The boot splash ends before the card opens | no | not the engine's: the session's start does it |
| 20-32 | The fallback colours before the shell's first render | unit | `the_default_chrome_is_what_chrome_rs_writes`; the rig's bare sessions run on them |
| 42-73 | The chrome defaults and the shell's published chrome | rig | `theme.set` in `session.sh`; the shell's own in `shell.sh` |
| 79-93 | The host defaults: the laptop panel, opacity 1, no lid output | unit | `an_empty_file_is_the_defaults` |
| 95-110 | The lid output is off while the lid is shut, also at a load | rig | `lid.sh` with a file standing in for `/proc/acpi/button/lid/LID0/state`: off, on, and off across a reload |
| 111-114 | The lid switch flips it | rig | `lid.sh`; the state is polled once a second, the core has no lid switch |
| 104-110 | A real lid | no | the rig has none |
| 119 | Any other display at its highest refresh rate | rig | `host.sh`: 800x600@60.317 picked over the preferred mode |
| 119 | Placed to the right of what precedes it | unit | `outputs_match_by_connector_then_description_then_the_catch_all`; one output on the rig |
| 122-123 | SUPER, the terminal | rig | `binds.sh`: SUPER+Return opens the host's terminal |
| 129-135 | The nine names and the host's workspace rules | rig, headless | `session.sh`, `host.sh`, headless |
| 142-151 | The environment, the host's on top | rig | `host.sh`: a spawned shell prints `nvidia direct Bibata-Modern-Classic qt6ct` |
| 157 | `kb_layout = es` | rig | `host.sh`: `keyboard.layouts` |
| 159 | `caps:escape` | rig | `misc.sh`: key 66 runs what Escape is bound to |
| 160-161 | Repeat 40 a second after 250 ms | rig | `binds.sh`: 24 or 25 runs in 900 ms held |
| 165 | `follow_mouse = 2` | rig | `rules.sh` |
| 168-182 | Pointer and touchpad: accel, sensitivity, natural scroll, clickfinger, typing, scroll factor | no | handed to the core's input factory; no libinput device on the rig |
| 190-191 | The gaps from the theme | rig | `session.sh` |
| 192 | The border, 2 px | rig | `session.sh` with `borderSize` 4: `16-theme` |
| 194 | The scrolling layout | rig | `tape.sh` |
| 195 | `resize_on_border` | rig | `rules.sh`: a float's edge and a tile's |
| 200-203 | Border colours from the theme | rig | `motion.sh`: 67 133 190 focused; `session.sh`: red |
| 207-213 | Group colours | no | there are no groups |
| 220-221 | Column width and presets | rig | `tape.sh` |
| 222 | A lone window is not stretched | rig | `tape.sh` |
| 225-226 | Fit, and the view follows the focus | rig | `tape.sh` |
| 234 | A vertical swipe goes on past the neighbour | headless | |
| 240 | A focus change leaves the pointer where it is | rig | `misc.sh`: 700,500 before and after |
| 244 | Rounding from the theme | rig | `session.sh` with rounding 0: `16-theme` |
| 248-255 | The shadow from the theme | rig | `misc.sh`: 40 41 43 without, 97 97 99 with a white one, `99-shadow` |
| 260 | `dim_special = 0.6` | rig | `motion.sh` |
| 264-265 | The host's opacity on every window | rig | `host.sh`; `misc.sh` at 0.8 |
| 266-271 | Blur behind what is translucent, size 8, two passes | rig | `misc.sh`: text behind a 0.8 window five times sharper with the theme's blur off; `97-blur`, `98-no-blur` |
| 273 | Animations off with the shell's motion | rig | `motion.sh` |
| 275-276 | No logo, no splash | rig | any screenshot |
| 279 | Black under everything | no | not met: the core clears to its own 20 26 33 (`CLEAR_COLOR` in `tty.rs`) |
| 284 | A window that asks for the focus gets it | rig | `misc.sh`: `wlrctl toplevel focus`, the view follows to its workspace |
| 285 | `anr_missed_pings` | no | not ported: the core has no dialog for a client that stopped answering |
| 288 | The lock is taken back by a restarted shell | no | the core's session lock; the backend agent's sweep proved lock and unlock on this build |
| 291 | VRR off unless an output asks | no | the core has no VRR; `vrr` in the host file is read and unused |
| 294 | Direct scanout per host | no | the core has no switch; `directScanout` is read and unused. A fullscreen window is given no frame so its buffer can reach the plane |
| 299 | Auto HDR | no | the core has no such mode |
| 306-308 | Xwayland renders at scale 1 | no | no Xwayland on the rig, no core setting |
| 318-322 | The DualSense touchpad: flat, 0.7 | no | handed to the core by device name; no such device on the rig |
| 329-346 | The animations | rig | the motion section |
| 346 | No zoom on a new output | rig | nothing animates an output: `lid.sh`, `overview.sh` |
| 357 | Three fingers sideways | headless | |
| 358 | Three fingers up and down | headless | |
| 360-415 | The pointer hidden until the shell arrives | rig | `rules.sh`: hidden at the start; `shell.sh`: shown once the shell asks |
| 445-447 | Shown after ten seconds without a shell | rig | `rules.sh`: hidden, then shown, the pointer's picture in the second shot |
| 417-434 | The pointer leaning and growing | no | not ported: a Hyprland plugin, the core draws the cursor untransformed |
| 438-444 | Xwayland's primary output | no | `xrandr` is run at start and when an output is added; no Xwayland on the rig |

### Binds (452 to 593)

Pressed through wtype with FormalShell running and a recorder standing in for
`formalshell`, `wpctl`, `helium`, `ghostty` and `systemctl` (`binds.sh`), or in
`tape.sh` through both `--key` and wtype.

| Lines | Chord | Check | Evidence |
| --- | --- | --- | --- |
| 452 | SUPER+Space, menu toggle | rig | `binds.sh` |
| 456 | SUPER+period, emoji | rig | `binds.sh` |
| 458 | SUPER+ntilde, clipboard | rig | `binds.sh` |
| 464, 533 | SUPER+plus, console toggle and +10% | rig | `binds.sh`: the command ran; `tape.sh`: 568 to 670 |
| 465 | SUPER+SHIFT+T, theme mode | rig | `binds.sh` |
| 469 | SUPER+SHIFT+E, logout | rig | `binds.sh` |
| 470 | SUPER+SHIFT+Escape, lock then suspend | rig | `binds.sh`: `lock lock`, then `systemctl suspend`, through sh |
| 482 | Print | rig | `binds.sh` |
| 483 | SUPER+SHIFT+S | rig | `binds.sh` |
| 491-492 | Volume up and down, locked, repeating | rig | `binds.sh`: 24 runs held, and they run locked |
| 493-494 | Mute, mic mute | rig | `binds.sh`: once held, not while locked |
| 495-496 | Brightness, locked, repeating | rig | `binds.sh` |
| 497-500 | Play, pause, next, previous | rig | `binds.sh` |
| 503 | SUPER+Return | rig | `binds.sh`: a terminal opens, and does not while locked |
| 507 | SUPER+SHIFT+F23 | rig | `binds.sh` |
| 508 | SUPER+Q | rig | `binds.sh`, `session.sh` |
| 509 | SUPER+SHIFT+F | rig | `tape.sh` |
| 510 | SUPER+V | rig | `tape.sh` |
| 511 | SUPER+B | rig | `binds.sh` |
| 517-518 | SUPER+H, L, wrapping | rig | `tape.sh` |
| 519-520 | SUPER+J, K | rig | `tape.sh` |
| 521-524 | SUPER+SHIFT+H, L, J, K | rig | `tape.sh` |
| 529 | SUPER+F | rig | `tape.sh`: `[8,8 1008x752]` |
| 530 | SUPER+M | rig | `tape.sh`: `[0,0 1024x768]` |
| 531 | SUPER+R | rig | `tape.sh` |
| 532 | SUPER+minus | rig | `tape.sh`: 670 to 568 |
| 538 | SUPER+C | rig | `tape.sh` |
| 539-540 | SUPER+comma, SUPER+SHIFT+comma | rig | `tape.sh` |
| 544-547 | SUPER+1 to 9, SUPER+SHIFT+1 to 9 following | rig | `tape.sh`: all nine; SUPER+SHIFT+4 |
| 549 | SUPER+Tab | rig | `tape.sh` |
| 554, 556 | SUPER+O, SUPER+E | rig | `session.sh`, `overview.sh`; the engine's own overview, not the shell's |
| 560 | SUPER+Z | rig | `tape.sh`; the engine's own zen |
| 569-570 | ALT+Tab, ALT+SHIFT+Tab, repeating | rig | `binds.sh`: `switcher next`, `switcher prev` |
| 575-576 | Letting Alt go commits | rig | `session2.sh` with the compositor's keys: `next`, then `commit` on release; `binds.sh` through wtype, which sends no Alt key: the commit runs at the next key without Alt |
| 586-587 | SUPER and the wheel | rig | `tape.sh`: the occupied workspaces, wrapping, both ways |
| 588-589 | XF86Tools, XF86Launch5 | rig | `tape.sh` |
| 592 | SUPER and the left button | rig | `session2.sh` |
| 593 | SUPER and the right button | rig | `session2.sh`: 512x461 to 592x501 |

### Window rules (598 to 688)

| Lines | Rule | Check | Evidence |
| --- | --- | --- | --- |
| 598 | Helium to 1 | rig | `rules.sh`; `heliumx` does not match |
| 599 | Code, Zed to 3 | rig | `rules.sh` |
| 600 | Slack, WhatsApp, discord, Beeper, Signal to 4 | rig, unit | `rules.sh`: Slack, Beeper; the others in `the_rules_place_the_apps_they_name` |
| 602 | The title Messages to 4 | rig | `rules.sh` |
| 605 | Beeper tiled although it maps floating | rig | `rules.sh` |
| 608 | Sushi floats centred at its own size | rig | `rules.sh`: `[362,284 300x200]` |
| 615-619 | The console on its overlay, floating, silent | rig | `session.sh` |
| 620-623 | Obsidian to 5, Claude to 7, Kopuz and formalmusic to 8 | rig | `rules.sh` |
| 625, 627 | Sober and Steam games as game content | no | the core has no content type; read and unused |
| 630-633 | An idle pointer hides after 3 s over a Steam game, and only there | rig | `rules.sh` |
| 636 | YouTube Music to 8 | rig | `rules.sh` |
| 643 | Minecraft confines the pointer | no | no Xwayland on the rig and no core hook for an X11 grab |
| 657-660 | Video picture in picture: float, pin, 28%, bottom right | rig | `rules.sh`: `[721,537 287x215]`, there on every workspace |
| 664-667 | A helium popup that maps floating: the same | rig | `rules.sh` |
| 671-673 | The built-in notification: float, pin, top right | rig | `rules.sh`: `[648,16 360x120]` |
| 680-683 | `nd-hello` popups without blur | no | no Xwayland on the rig; the matcher in `tag_and_xwayland_matchers` |
| 688 | Atoll's tag floats | no | the core reports no toplevel tag; the matcher is unit tested |
| | A dialog of no rule floats at its own size | rig | `rules.sh`: `[352,284 320x200]` |

### Layer rules (697 to 716)

| Lines | Rule | Check | Evidence |
| --- | --- | --- | --- |
| 697-702 | Bar, tooltip, osd, switcher: blur from alpha 0.2 | rig | `layers.sh`: bar and osd at 0.10 sharp, at 0.40 frosted; tooltip and switcher are the same rule |
| 706-714 | Menu, polkit, plugin-overlay, radio, panel, notifications: blur from 0.6 | rig | `layers.sh`: menu and notifications at 0.40 sharp, at 0.70 frosted |
| 716 | The veil: no blur | rig | `layers.sh` |
| | A namespace the settings do not name: no blur | rig | `layers.sh` |
| | The theme's blur off: none | rig | `layers.sh` |
| 697-716 | `no_anim` | no | nothing animates a layer |

## The g815's host.lua

| What | Check | Evidence |
| --- | --- | --- |
| Three outputs by description, mode, position, scale | unit | `g815_lays_its_outputs_over_the_shared_file`, `a_hosts_outputs_come_before_the_shared_ones`; the displays are not on the rig |
| Workspaces 4 and 8 on the portable, the rest on the desk, 1 and 4 in view | headless, unit | two test outputs with the same pins |
| An output that goes takes its workspaces to the first, and gets them and its view back | rig, unit | `lid.sh`: workspace 2 in view again; `an_output_that_returns_shows_what_it_showed_and_takes_its_workspaces_back` |
| The lid output | rig | `lid.sh` |
| `xwayland_primary` | no | no Xwayland on the rig |
| Opacity 0.96 | rig | `host.sh` reloaded onto the g815's file |
| `direct_scanout = 0`, `vrr` | no | no core setting |
| The environment | rig | `host.sh` |

## Motion

Each animation, the `hyprland.lua` leaf it replaces and the family it rides.
Read from the engine's frame log in `motion.sh`, and from `core/tests/scene.rs`.

| What moves | Leaf | Family | Evidence |
| --- | --- | --- | --- |
| A window's place and size in the tape: scroll, width, swap, stack, zen | `windows` | spatial | a scroll passes its place by 15 px and comes back; `a_spatial_spring_overshoots_as_far_as_its_curve` |
| A window opening: from a point at its centre | `windows` | spatial | width from 5 px to 500 |
| Its opacity coming in | `fade` | effects | 0 to 100, no overshoot; `an_effects_spring_never_overshoots_and_arrives_on_time` |
| A window closing: shrinks to 90% | `windowsOut` | spatialFast | `90-closing` with the fast clocks slowed to two seconds |
| Its opacity going out | `fadeOut` | effectsFast | `90-closing` |
| The border colour | `border` | effectsSlow | 73 120 161 on the way, 67 133 190 landed |
| A workspace switch, vertical | `workspaces` | spatial | `one y` frame by frame |
| The console dropping in | `specialWorkspace` | spatial | y from -728 past 40 to 51 and back to 40 |
| The dim under it | | follows the console's way in, clamped | 0.6 at rest |
| The overview's zoom and its wash | | spatial | progress 0 to 101 to 100, the wash clamped to 0..1 |
| The drag card: scale in, fly-back | | spatialFast, spatial | `drags.sh`: `75-flying` is the fly-back mid-way |
| The drag card: fade | | effectsFast | by the code only, no frame of it is measured |
| A tape scrolled by a held drag | | none: 1400 px a second while held | `drags.sh` |
| Layers | `layers`, `layersOut` | none | nothing animates a layer |
| A new output | `monitorAdded` | none | |

Cut short mid-flight, each one goes on from where it is drawn, with its
speed, and never jumps (`motion.sh`, largest step between two frames against
the same move run whole):

| Move | Cut short by | Result |
| --- | --- | --- |
| A tape scroll | a second scroll 50 ms in | continuous, largest step 104 px |
| A workspace switch | a second switch 50 ms in | continuous, largest step 156 px |
| The overview opening | closing 120 ms in | turns back from where it was |
| Zen | zen off 50 ms in | the columns turn back, no jump |
| A width change | the next preset 50 ms in | continuous |
| The console dropping in | park, then show 120 ms later | turns back from where it was |
| A window opening | its first commit | starts again before anything is drawn |
| A closing window | | not interruptible: the core animates a snapshot of a window that is gone |

Also in `retargeting_mid_flight_keeps_the_velocity`,
`a_tween_restarts_from_where_it_is`,
`a_window_that_changes_place_starts_from_where_it_is_drawn`.

With `motion.enabled` false every one of them lands in a single frame
(`motion.sh`: switch, scroll, overview, zen, open, close, console;
`with_motion_off_everything_lands_at_once`).

### Frame times

Software GL on vkms: these are not hardware numbers, they are what the next
build is compared with on the same rig. The interval is between two frames the
engine presented; render and CPU time are the core's own
(`SHOJI_ANIMATION_TIMING_DEBUG`), the present rate its `SHOJI_PRESENT_RATE_DEBUG`.

| Move | Frames | Interval ms (median, p95, worst) | Render ms | CPU ms | Present rate |
| --- | --- | --- | --- | --- | --- |
| A tape scroll, four columns | 41 | 16.5, 21.4, 22.7 | 4.9, 7.2, 11.5 | 5.4, 7.5, 11.8 | 60 Hz |
| The overview opening | 41 | 18.2, 24.8, 30.4 | 4.4, 12.5, 19.2 | 4.7, 12.8, 19.4 | 32 Hz in the second it began |

## What the core does not offer

Each is read from the settings where the owner's config has it, and unused
until the core does.

- A lid switch. The engine polls the state file once a second (`lid.rs`).
- A modifier's release after a chord. The engine holds an input grab from
  ALT+Tab until Alt is let go (`binds.rs`).
- A drop target that follows the windows under a still pointer: a data drag
  let go at a tape edge without moving on lands in the window that was there
  before the tape scrolled (`drags.sh` shows it).
- Opacity or animation of a layer surface.
- The window's opacity kept off the blur behind it: at opacity a, the sharp
  desk shows through at (1 - a) squared. 0.16% at the g815's 0.96.
- The clear colour (line 279).
- VRR, the direct scanout switch, content type, auto HDR, Xwayland's scale,
  confinement of an X11 pointer grab, the toplevel tag, cursor transforms,
  outputs without a connector.
