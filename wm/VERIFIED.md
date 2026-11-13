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
virtual pointer), one connector, which can do neither VRR nor HDR, no lid and
no Xwayland. A second output is one `outputs.create` makes. What needs more
than that is headless, unit or **no** below.

Sessions and what they need: `session.sh`, `session2.sh`, `tape.sh`,
`rules.sh`, `layers.sh`, `drags.sh`, `overview.sh`, `outputs.sh`, `focus.sh`
run bare.
`host.sh` with `FWM_HOST=host-default.json`, `misc.sh` with
`FWM_HOST=host-misc.json`, `motion.sh` and `cursor.sh` with `FWM_TIMING=1`, `shell.sh` with
`FWM_SHELL` naming FormalShell, as `handover.sh`, `binds.sh` with
`FWM_KEYS=wtype` and
`FWM_SHELL`, `locked.sh` with `FWM_LOCKED=1`, once bare and once with
`FWM_SHELL`.

Every rig, headless and unit line was run again on one build, the published
one: `shoji` e7c0b375 on the fork's `formal-rs` e6493f8, which is `formal` at
the same commit, 9757796 with upstream's main merged into it. `formalwm`
sha256 `a5a825cc6ab7489b010d70e4bce65b7080fe09921d8cf85d4ffc3726417a6115`.
All 19 sessions end with exit 0 on it. The numbers quoted are those of the
run before it, on 3395202f and `formal-rs` 9757796 (sha256
`cf1e99a7abce4d8d7215ddde19b03bdd1d02d001f873035e3b43ed59d8712bb9`), with
`session2.sh` and `motion.sh` from a second run on that build: the host was
under other builds during the first, in which the overview's corner took
longer than the session waits for it and `motion.sh` rendered at 24 Hz.
Compared line by line with that run, and with the S9 run on 279becde for the
new step of `outputs.sh`, this one differs only in what a run times: frame
counts and intervals, key repeat counts, the order the two commands of one
chord are recorded in, how soon the shell is up. `drags.sh` was run twice on
it: in the first the screenshot after a refused drop was taken before the
card's first frame on its way back (`left the hand: 0.0`), in the second it
reads 15.97, against 18.77 before. The default border is 2 px and every rect
below is a window's content, so the first column starts at x 10: 8 of gap
and 2 of border.

## Spec 2: the engine

| Behaviour | Check | Evidence |
| --- | --- | --- |
| `formalwm` is the compositor, on `shojiwm_rs`, in its own workspace | rig | every session starts it on the tty |
| Settings are data in `~/.config/formalwm/`, read at start | rig | `session.sh` start: the nine names, 73 binds; `host.sh`: the host file's pins and mode |
| Read again on the control socket's `reload` | rig | `session.sh`, `host.sh`: `config.reloaded`, new binds and outputs in force |
| A bad file keeps the running settings and reports the error | rig, unit | `session.sh`: `config.failed` for an action the engine has not, a key no keymap has (`shortcut \`Super+Retrun\` contains unknown key \`Retrun\``), an env name and an env value that cannot be set (`env: \`A=B\` cannot be set`); windows unmoved, SUPER+Return still opens a terminal; `an_env_name_or_value_that_cannot_be_set_is_refused`, `a_shortcut_the_core_cannot_read_is_refused` |
| The shell's settings format: comments, trailing commas | unit | `comments_and_trailing_commas_are_allowed`, `the_shared_file_loads_on_its_own` |
| The host file lays over the shared one | rig, unit | `host.sh`; `the_host_file_lays_over_the_shared_one`, `g815_lays_its_outputs_over_the_shared_file` |
| Wire types are `crates/fs-wm-proto` | unit | its nine tests read and write the spec's JSON |
| Columns of stacked windows | rig | `tape.sh`: `[10,10 496x368]` over `[10,390 496x368]`; `session.sh`: `05-stack` |
| Default column width 0.5 | rig | `tape.sh`: 496 of 1024 less the gaps and the border |
| Presets 0.333, 0.5, 0.667, 1.0 | rig, unit | `tape.sh`: SUPER+R to 666, SUPER+F to 1004; the wrap to 0.333 in `presets_cycle_upward_and_wrap` |
| A lone column keeps its width and is centred | rig | `tape.sh`: `[264,10 496x748]` |
| Focus fits the column by the least scroll, no recentring | rig, unit | `tape.sh`: focus three moves nothing, focus two scrolls one column; `focus_fits_by_the_least_scroll_and_does_not_recentre` |
| 8 px between everything, from the theme | rig, unit | `tape.sh`: x 10 and 518, 496 wide; `session.sh` with `gapsIn` 8, `gapsOut` 16 and `borderSize` 4: `[20,20 480x728]`, `[524,20 480x728]`; `eight_pixels_between_everything` |
| Gaps are measured to the outer edge of a window's border | rig, unit | `tape.sh` at border 2: 8 from the output's edge to the border, 8 between two borders (one content ends at 506, the next starts at 518, less 2 and 2); `shell.sh` under the shell's gap of 6 and border of 1: `[7,41 500x720]` below a bar reserving 34; `session2.sh`: 154 reserved on the left puts the window at x 164, on the right its far edge at 860; `gaps_are_measured_to_the_outer_edge_of_a_window_s_border` (border 1 and 2: the outer gap, between columns, between stacked windows), `the_outer_gap_lies_inside_what_a_layer_reserves_at_the_side` |
| A maximised window keeps its border in view | rig, unit | `tape.sh`: SUPER+M is `[2,2 1020x764]`; the same test. Zen and fullscreen are as they were |
| Nine workspaces with the spec's names | rig | `session.sh` start |
| Stacked vertically, a switch slides vertically | rig | `motion.sh`: `one y` frame by frame from 10 to -758; `session.sh`: `08-switching` |
| A workspace pinned to an output by the host settings | rig, headless, unit | `host.sh` on the one output (2, 3, 4 pinned, 3 in view at the start); `outputs.sh`: 4 and 8 move to a made output on a reload, and Slack opens there; headless: stacks `1 2 3 5 6 7 9` and `4 8`; `workspaces_pinned_by_the_host_live_on_their_output` |
| Focus changes on click only | rig | `rules.sh`: the pointer over another window leaves the focus; `session2.sh`: a click moves it |
| The output under the pointer is the focused one, the keyboard staying | rig, headless, unit | `outputs.sh`: the pointer at 1700,400 on a made output right of the first, `focusedOutput` `HEADLESS-1` and its workspace `d1`, back at 500,400 `Virtual-4` and `1`; headless: one `state.changed` with `focusedOutput` `TEST-1`, `focusedWindowId` and the seat's keyboard still on the window of `TEST-2`; a press on the empty output reaches it too; nothing while a window is dragged across; `the_pointer_takes_the_outputs_focus_and_leaves_the_keyboard` |
| `focusedWindowId` is the window that has the keyboard | rig, headless | `focus.sh`: of two windows neither used, the older one picked through `window.focus` hears the A and the state names it, a press on the newer moves both, and back and forth, SUPER + L and SUPER + H move both one way and back, a window that maps takes both, its closing gives both back to one window, and an empty workspace leaves the state naming none while no window hears the A; the session fails on any press where the window that heard it is not the one `state.get` names; headless: a focus the core refuses leaves the model within two ticks, and the same pick and press |
| A workspace in view under the pointer, asked for, takes the keyboard | rig, headless, unit | `outputs.sh`: the keyboard on a window of `HEADLESS-1` and the pointer on `Virtual-4`, `workspace.focus` of its empty workspace 1 and the window hears no more keys, the state naming none; a window on 1 and the pointer on `HEADLESS-1`, `workspace.focus` of `d1` and its window hears the A, the state naming it; headless: the same over the socket with a workspace that holds a window and one that holds none; `a_workspace_in_view_under_the_pointer_takes_the_keyboard_when_asked_for`, which has `workspace.focusAt`, the last workspace and a focus bind too |
| The hovered window still gets pointer events | rig | `rules.sh`: the unfocused client prints `hover` |
| Every bind in `hyprland.lua`, same chords | rig | the bind table below |
| A chord runs every bind it has, each by its own flags | rig, unit | `binds.sh`: a second bind on the volume key, not marked `locked` and not `repeat`, runs beside the first, once while the key is held for 23 runs of the first, and not on the lock screen; `on_the_lock_screen_a_chord_runs_only_its_binds_marked_locked`, `each_bind_of_a_shortcut_keeps_what_it_says_of_itself` |
| A bind on a modifier let go runs after the modifier alone; after a chord only when it is `passthrough` and holds its modifier | rig, headless, unit | `binds.sh`: after Alt+Tab the `passthrough` bind on `ALT+Alt_L` commits the switcher and neither the plain one nor the `passthrough` one on a bare `Alt_L` runs; Alt tapped alone runs all three; headless: a bare `Control_L` bind runs after Ctrl alone and not after Ctrl+A, one on `SHIFT+Shift_L` after Shift+A too; `a_bare_modifier_let_go_fires_after_the_modifier_alone` |
| The `mac` submap: a Mac window is handed the Super chords | rig, headless, unit | `handover.sh` with the shell: held over a tagged window with `SUPER + H` and `SUPER + L` reserved, Super+Z and Super+Shift+F reach it (`125 44`, `125 42 33`) and zen and fullscreen do not run; Super+H is still the bind's, and over the other window Super+Z is zen again; the keyboard back, the chords are the window's with nothing asked again; XF86Tools stays bound; every bind is back under a layer that takes the keyboard, under the overview and on the lock screen; it ends on `on` false, when the asking connection closes and when the window does; a reload keeps it; headless the same without the lock; `super_alone_and_with_shift_go_to_the_window`, `a_reserved_chord_stays_bound_with_the_modifiers_it_names` and the other four tests of `handover.rs` |
| The es number row by keycode | rig | `tape.sh` through `--key` and again through wtype typing the es digits; SUPER+SHIFT+4 too |
| Three fingers sideways drag the tape 1:1 | headless | scroll 716 for 300 px of fingers from 1016 |
| A release flings and snaps to a column | headless, unit | rests on a column edge with the focus in view; `a_released_scroll_snaps_a_column_edge_to_a_view_edge` |
| Three fingers up or down walk workspaces, past the neighbour | headless, unit | held at half a workspace, a flick lands two away; `a_held_stack_follows_the_fingers_past_the_neighbour` |
| Four fingers up open the overview | headless | progress 0.5 under the fingers, open on release |
| The same on a real touchpad | no | the rig has none |
| Rule matchers: app id, title, floating | rig | `rules.sh` |
| Rule matcher: toplevel tag | rig, unit | `rules.sh`: a window tagged `atoll` floats, the same app untagged tiles; `tag_and_xwayland_matchers` |
| A window's toplevel tag in its row | rig, headless | `handover.sh`: `atoll-mac` on the tagged window, `""` on the other; headless: a tag the client changes is broadcast |
| Rule matcher: xwayland | unit | `tag_and_xwayland_matchers`; no Xwayland on the rig |
| Rule effects: workspace, float, pin, size, position | rig | `rules.sh`, the rule table below |
| Rule effect: `confinePointer` | rig, headless, unit | `cursor.sh`: over a window of `[264,10 496x748]` whose rule sets it, a mouse pushed 3000 px stops at 760, at 758 and at 264, 10, and runs on to 1024 once another window has the keyboard; headless: held at the window's edge on the second output, free when another window opens; the matcher in `the_rules_place_the_apps_they_name` |
| Rule effect: opacity | rig | `rules.sh`: a rule of 0.5 added by a reload; a window of 42 74 106 reads 148 164 180 over white and 21 37 53 over the black desk, half of each (`71-opacity`). No rule of the owner's sets one |
| Motion on the six families | rig, unit | the motion section below |
| The dynamic cursor: stretched along the way it travels | rig, headless, unit | `cursor.sh`, the pointer's picture cut out of a screenshot against the bare desk: 16x24 at rest; 26x17 gliding right at 1600 px/s (told 1.8 along, 0.6 across, axis 0), 15x26 early in a glide down at 1500 (told up to 1.73, 0.64, axis 90), 34x39 down and right at 1770 (1.85, 0.57, axis 45), never rotated, 16x24 again a second later; headless: the core holds a transform while the pointer moves and none at rest; `a_pointer_at_speed_stretches_along_its_way_and_stays_upright`, `the_speed_s_share_of_the_full_effect` |
| Shaken, it grows, and stays grown two seconds | rig, unit | `cursor.sh`: 128x192 after fourteen strokes, 16x24 four and a half seconds on; `a_shaken_pointer_grows_and_goes_back_after_the_timeout`. The growth stops at 8 times, the core's bound, where the plugin has none |
| A change of `cursor.dynamic` drops what was begun under the old settings | rig, unit | `cursor.sh`: shake turned off by a reload while the cursor is magnified leaves the image 16x24 and the core renders 0 frames in the two seconds after, where it rendered 64 in the second and a half before; the build before this one rendered 105 after. `other_settings_drop_what_was_begun_under_the_last`: shake off, the whole of it off, another mode mid-motion |
| `cursor.dynamic`: `tilt`, `none`, shake off, what cannot be | rig, unit | `cursor.sh` after reloads: tilt turns the image up to 12 degrees right and -16 left with no stretch, `none` with shake off tells the core nothing, a limit of 0 is `config.failed`; `a_tilt_leans_into_sideways_travel`, `the_settings_default_to_the_owner_s_and_refuse_what_cannot_be`; the example file's values in `the_shared_file_loads_on_its_own` |
| No shape while the cursor is hidden or the session locked, none at rest | rig, headless | `cursor.sh`: a hidden pointer's glide tells the core nothing, and every glide ends on no transform; `locked.sh` with the shell: a sweep across the lock screen tells nothing, the same sweep after the unlock 15 to 29 shapes from run to run |
| The same on a hardware cursor plane | no | the rig draws the cursor in software; needs the g815. The plugin pivots the stretch at the leading edge of the image, the core at the hotspot |
| Blur and alpha threshold per layer namespace | rig | `layers.sh` |
| No compositor animation on a `formalshell:*` layer | no | nothing to observe: the engine animates no layer and the core has no layer animation. `animate` is read (`a_layer_rule_by_namespace`) |
| A settings reload keeps every window where it is | rig, unit | `session.sh`: same workspaces and columns after `reload`; `a_settings_reload_keeps_every_window_where_it_is` |
| Session start runs `formalshell-start.sh` once the socket is up | rig | `session2.sh`: the script wrote `display wayland-1, socket up` |
| `FORMALWM_START_LOCKED=1`: nothing is drawn before the lock | rig | `locked.sh` with the shell: the start script takes 70 screenshots as fast as grim can from the moment it runs. The first six are black, 0 0 0, then the lock's own frames, secure 137 ms after the first; none has the window the script started, though `state.get` lists it |
| The shell's lock comes up and the password opens the desktop | rig | `locked.sh` with the shell: `lock status` secure with `boot` true, `85-locked` without a pixel of the window; the password typed through wtype, `86-unlocked` with the window and the shell's six layers |
| The start script runs while locked | rig | `locked.sh`, both ways: `display wayland-1, socket up`, and the shell it starts is what locks |
| Locked and no shell: it stays dark and does not unlock | rig | `locked.sh` bare: all 70 frames black, also at 13 s (`87-dark`, and `88-dark-with-pointer`: the pointer stays hidden too), SUPER+Return opens nothing |
| Without the variable nothing changes | rig | every other session starts without it and draws at once |
| `shojiwm_rs` and `shojiwm_lib` by git rev | rig | `formalwm/Cargo.toml` names a rev of the fork's `formal`; a fresh clone of this branch builds with `--locked` in the container, with no ShojiWM checkout beside it |

## Spec 3: the socket

| Behaviour | Check | Evidence |
| --- | --- | --- |
| The JSON socket at `$XDG_RUNTIME_DIR/shojiwm-$WAYLAND_DISPLAY.sock`, newline JSON | rig | every `ipc` call of every session |
| Request, answer, error, broadcast envelopes | rig, unit | `session2.sh`: `no such window: no-such-window`; `the_envelope_is_shojiwms` |
| `$SHOJIWM_SOCKET` is never bound by the engine | rig, unit | `reload` written to `$SHOJIWM_SOCKET` works in `session.sh`, `host.sh`, `binds.sh`; `the_json_socket_is_not_the_control_socket` |
| `state.changed` once per tick, dropped when unchanged | rig, unit | `session.sh`: one broadcast across one focus change, none in the idle seconds around it; `the_same_model_publishes_the_same_json` |
| `State.outputs`, logical | rig | `session2.sh`: 512x384 at scale 2 |
| `outputs[].reserved` | rig, unit | `session2.sh`: top 36 under a bar, and 154 or 148 at the side a layer reserves there; `shell.sh`: 34 under the shell's bar on both outputs; `an_output_says_what_its_layers_reserve` |
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
| `chrome`, with `borderSize` | rig, unit | `session.sh`: `gapsOut` 16, `borderSize` 4, `activeBorder` `#ff0000`; `shell.sh`: the shell's own, `borderSize` 1; `the_chrome_says_the_border_the_gaps_are_measured_to` |
| `workspace.focus` | rig | `session.sh` |
| `workspace.focusAt` | rig, unit | `session2.sh` by idx; with `output` in `workspaces_by_stack_position_and_by_neighbour` |
| `window.focus` | rig, headless | `motion.sh`, `drags.sh`; `focus.sh` and headless: the keyboard goes to an older window over a newer one |
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
| `bind.escape` | rig, headless | `session2.sh`: runs once while set, not after; `misc.sh`; `focus.sh` and headless: the command runs and the window still hears the Escape (key 1), unbound it hears it and nothing runs |
| `binds.list` | rig, headless | `session.sh`: 73 rows, `mods` in the wire order, `code:10`; `session2.sh` and headless: a row for the shell's Escape while `bind.escape` is set, gone after |
| `binds.handOver` | rig, headless | `handover.sh`; a `window` that is none answers `no such window: nobody`; `State.handOver` follows each step with `window`, `reserved` as given and `active` |
| `cursor.pos` | rig | `session2.sh`, `misc.sh` |
| `cursor.hide` | rig | `session2.sh` |
| `layers.list` | rig | `session2.sh`: three layers bottom first with boxes and pids; `shell.sh`: the shell's own, and with a made output each one once under its output |
| `outputs.get` | rig | `session2.sh`: 34 modes, 60.004 Hz; `overview.sh`: zeros when disabled; `outputs.sh`: a made output lists its one mode |
| `outputs.set` `scale` | rig | `session2.sh`: 2 and back, `30-scale2` |
| `outputs.set` `enabled` | rig | `overview.sh`: off, on, every window where it was; `outputs.sh` on a made output |
| `outputs.set` `width`, `height` | rig, unit | `shell.sh`: a made output of 800x600 set to 1600x900 is that size in `State.outputs`, the shell's wallpaper and bar on it are told 1600 wide and none is closed or made twice; refused on `Virtual-4`; one field alone and what is refused in `outputs_set_resizes_only_an_output_outputs_create_made` |
| `outputs.set` `mirror`, set and cleared | rig | `outputs.sh` on a made output: the core is given the mirror and its source, `mirrorOf` reports it, and clearing it brings the output back with its workspace and window |
| The mirrored picture | no | the rig's only second output has no connector, and the core mirrors onto a scanout: the made output's capture shows the bare desk while it mirrors (`52-mirror`) |
| `outputs.set` `color` | no | vkms has no HDR. The core is given HDR on for a `cm` other than `srgb` and its automatic mode otherwise (line 299 below); `bitdepth`, `sdrBrightness` and `sdrSaturation` have no core setting (it takes SDR white in cd/m² and a gamut, not Hyprland's factors) |
| `outputs.create` | rig | `outputs.sh`: answers the name, right of the others at y 0, a workspace of its own, one `state.changed`, a window drawn on it (`50-made-output`), kept across a reload; a taken name and a size of 0 are refused |
| `outputs.remove` | rig | `outputs.sh`: its workspaces and windows go to the output that is left; an output with a connector and an unknown name are refused; `shell.sh`: its layers are gone from `layers.list` and none is left on the other output, also when it is made again under its name |
| `theme.set` | rig | `session.sh`, `motion.sh`, `misc.sh`, `layers.sh` |
| The last theme stands across a settings reload | rig | `overview.sh` |
| `session.exit` | rig | `session2.sh`: socket gone, no `formalwm` left |
| `config.reloaded`, `config.failed` | rig | `session.sh` |
| `Output.vrr` | rig | `misc.sh`: false, true while a window is fullscreen under `vrr` 2, false again. It is what the engine asks of the core; the rig's connector cannot do VRR, and the core says so in the log each time it is asked |
| `Output` and `Theme` as the spec writes them | unit | `an_output_row_and_its_colour_read_as_the_spec_writes_them`, `a_partial_theme_keeps_the_defaults_for_the_rest`, `the_default_chrome_is_what_chrome_rs_writes` |

## Spec 4: overview

| Behaviour | Check | Evidence |
| --- | --- | --- |
| The output zooms out to its workspaces stacked, each its whole tape, live windows | rig | `session.sh`: `11-overview`, `12-overview-down` |
| The shell's layers stay where they are | rig | `shell.sh`: `41-shell-overview` |
| Opens on the bind | rig | `session.sh`: SUPER+O; `overview.sh`: SUPER+E |
| Opens on `overview.set` | rig | `motion.sh`, `overview.sh` |
| Opens on four fingers up, closes on four down | headless | |
| Opens from the shell's hot corner | rig | the shell's `--overview` under formalwm: the corner parked opens it and it stays, a leave and a return close it; the bind, IPC and the minimap's press open and close it the same |
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
| The zen window takes the zen size, centred | rig | `tape.sh`: `[12,46 1000x676]`; `session.sh`: `10-zen` |
| Every other column slides off the side it was on | rig | `tape.sh`: -1522, -1014 to the left, 2050 to the right |
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
| Held at a tape edge, the tape scrolls | rig | `drags.sh`: 941 px on a second after the hand set off, and back, with data and with a window in the hand |
| Held over a window, that window is focused and the drop lands in it | rig | `drags.sh`: the text arrives in the terminal and the keyboard follows, `73-dropped` |
| SUPER and the left button lift a window out of the tape | rig | `session2.sh`: `21-dragging` |
| Dropped into the column gap under the pointer | rig, unit | `session2.sh`: `22-dropped`; `a_drop_lands_in_the_gap_under_the_pointer` |
| One window of a stack in the hand leaves the rest of its column | rig, unit | `drags.sh`: of two stacked in column 2 the one dragged lands in a column of its own, the other fills the column it stays in; `one_window_moved_out_of_a_stack_leaves_the_rest_of_its_column` |
| Dropped onto a workspace in the overview | rig | `session2.sh`: `26-dropped-in-overview` |

## hyprland.lua, top to bottom

| Lines | What | Check | Evidence |
| --- | --- | --- | --- |
| 1-8 | The boot splash ends before the card opens | no | not the engine's: the session's start does it |
| 20-32 | The fallback colours before the shell's first render | unit | `the_default_chrome_is_what_chrome_rs_writes`; the rig's bare sessions run on them |
| 42-73 | The chrome defaults and the shell's published chrome | rig | `theme.set` in `session.sh`; the shell's own in `shell.sh` |
| 79-93 | The host defaults: the laptop panel, opacity 1, no lid output | unit | `an_empty_file_is_the_defaults` |
| 95-110 | The lid output is off while the lid is shut, also at a load | headless | the core's lid switch toggled shut: the lid output is disabled and its workspaces go to the other. The output factory reads the switch's last report, which is what a load and a reload run |
| 111-114 | The lid switch flips it | headless | shut, as above. Open is not seen there: the harness stops listing an output it has disabled, so the way back on is the factory's enabled path, which `overview.sh` and `outputs.sh` run on the rig through `outputs.set` |
| 104-110 | A real lid | no | the rig has no switch and nothing there can send one |
| 119 | Any other display at its highest refresh rate | rig | `host.sh`: 800x600@60.317 picked over the preferred mode |
| 119 | Placed to the right of what precedes it | rig, unit | `outputs.sh`: a made output, which only the catch-all entry matches, sits at x 1024, the next at 2304; `outputs_match_by_connector_then_description_then_the_catch_all` |
| 122-123 | SUPER, the terminal | rig | `binds.sh`: SUPER+Return opens the host's terminal |
| 129-135 | The nine names and the host's workspace rules | rig, headless | `session.sh`, `host.sh`, headless |
| 142-151 | The environment, the host's on top | rig | `host.sh`: a spawned shell prints `nvidia direct Bibata-Modern-Classic qt6ct` |
| 157 | `kb_layout = es` | rig | `host.sh`: `keyboard.layouts` |
| 159 | `caps:escape` | rig | `misc.sh`: key 66 runs what Escape is bound to |
| 160-161 | Repeat 40 a second after 250 ms | rig | `binds.sh`: 23 to 25 runs in 900 ms held |
| 165 | `follow_mouse = 2` | rig | `rules.sh` |
| 168-182 | Pointer and touchpad: accel, sensitivity, natural scroll, clickfinger, typing, scroll factor | no | handed to the core's input factory; no libinput device on the rig |
| 190-191 | The gaps from the theme | rig | `session.sh` |
| 192 | The border, 2 px | rig | `session.sh` with `borderSize` 4: `16-theme`, the rects 4 further in; `chrome.borderSize` says the width |
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
| 248-255 | The shadow from the theme | rig | `misc.sh`: 34 34 34 without, 92 92 92 with a white one, `99-shadow` |
| 260 | `dim_special = 0.6` | rig | `motion.sh` |
| 264-265 | The host's opacity on every window | rig | `host.sh`: a terminal's 36 36 36 reads 45 45 45 at 0.96 over white; `rules.sh` at 0.5 |
| 266-271 | Blur behind what is translucent, size 8, two passes | rig | `misc.sh`: text behind a 0.8 window has sharpness 0.12 blurred, 0.53 with the theme's blur off, 19.81 beside the window (`97-blur`, `98-no-blur`); `rules.sh`: six pixels from a white strip's edge behind a 0.5 window, 49 65 81 blurred against 21 37 53 |
| 273 | Animations off with the shell's motion | rig | `motion.sh` |
| 275-276 | No logo, no splash | rig | any screenshot |
| 279 | Black under everything | rig, headless | the core's clear colour, set at the start and again after a settings reload: `session.sh` reads the bare desk 0 0 0 after its reloads, `outputs.sh` the same at a made output's edge, `locked.sh` 70 black frames; headless: black at the start and after a reload |
| 284 | A window that asks for the focus gets it | rig | `misc.sh`: `wlrctl toplevel focus`, the view follows to its workspace |
| 285 | `anr_missed_pings` | no | not ported: the core has no dialog for a client that stopped answering |
| 288 | The lock is taken back by a restarted shell | no | the core's session lock; the backend agent's sweep proved lock and unlock on this build |
| 291 | VRR off unless an output asks | rig, headless, unit | every bare session: `Output.vrr` false and the core never asked; `vrr` 2 and 3 as in the host table below; the levels 0 to 3 in `outputs_match_by_connector_then_description_then_the_catch_all` |
| 294 | Direct scanout per host | headless | `directScanout` false in the host file reaches the core's setting for every output. What a plane then shows needs hardware |
| 299 | Auto HDR | headless | every output is given the core's automatic mode: SDR until a fullscreen window shows HDR content. The switch itself needs an HDR panel, the g815's |
| 306-308 | Xwayland renders at scale 1 | no | no Xwayland on the rig, no core setting |
| 318-322 | The DualSense touchpad: flat, 0.7 | no | handed to the core by device name; no such device on the rig |
| 329-346 | The animations | rig | the motion section |
| 346 | No zoom on a new output | rig | nothing animates an output: `outputs.sh`, `overview.sh` |
| 357 | Three fingers sideways | headless | |
| 358 | Three fingers up and down | headless | |
| 360-415 | The pointer hidden until the shell arrives | rig | `rules.sh`: hidden at the start; `shell.sh`: shown once the shell asks |
| 445-447 | Shown after ten seconds without a shell | rig | `rules.sh`: hidden, then shown, the pointer's picture in the second shot |
| 417-434 | The pointer leaning and growing | rig, headless, unit | `cursor.dynamic` in the settings, with the plugin's values as `hyprland.lua` sets them: `stretch`, the tilt's 20 degrees at 3000 px/s, shake on. The rows of spec 2 above; on a hardware cursor plane: no, needs the g815 |
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
| 464, 533 | SUPER+plus, console toggle and +10% | rig | `binds.sh`: the command ran; `tape.sh`: 564 to 666 |
| 465 | SUPER+SHIFT+T, theme mode | rig | `binds.sh` |
| 469 | SUPER+SHIFT+E, logout | rig | `binds.sh` |
| 470 | SUPER+SHIFT+Escape, lock then suspend | rig | `binds.sh`: `lock lock`, then `systemctl suspend`, through sh |
| 482 | Print | rig | `binds.sh` |
| 483 | SUPER+SHIFT+S | rig | `binds.sh` |
| 491-492 | Volume up and down, locked, repeating | rig | `binds.sh`: 23 and 24 runs held, and they run locked |
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
| 529 | SUPER+F | rig | `tape.sh`: `[10,10 1004x748]` |
| 530 | SUPER+M | rig | `tape.sh`: `[2,2 1020x764]`, the border in view |
| 531 | SUPER+R | rig | `tape.sh` |
| 532 | SUPER+minus | rig | `tape.sh`: 666 to 564 |
| 538 | SUPER+C | rig | `tape.sh` |
| 539-540 | SUPER+comma, SUPER+SHIFT+comma | rig | `tape.sh` |
| 544-547 | SUPER+1 to 9, SUPER+SHIFT+1 to 9 following | rig | `tape.sh`: all nine; SUPER+SHIFT+4 |
| 549 | SUPER+Tab | rig | `tape.sh` |
| 554, 556 | SUPER+O, SUPER+E | rig | `session.sh`, `overview.sh`; the engine's own overview, not the shell's |
| 560 | SUPER+Z | rig | `tape.sh`; the engine's own zen |
| 569-570 | ALT+Tab, ALT+SHIFT+Tab, repeating | rig | `binds.sh`: `switcher next`, `switcher prev` |
| 575-576 | Letting Alt go commits | rig | `binds.sh` through wtype: `switcher next`, then `switcher commit` when Alt goes, no key after it; Tab twice under Alt steps twice before the one commit; while Alt is held another window hears the pointer and a click focuses it. The compositor's `--key` presses and lets go at once: both commands run, in the order their two processes reach the recorder. A second bind on Alt let go, without `passthrough`, runs only when Alt is tapped with no key between. A release bind of a reloaded file in `session2.sh` |
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
| 625, 627 | Sober and Steam games as game content | rig, unit | `misc.sh` under `vrr` 3: fullscreen, `steam_app_1` is asked VRR by its rule, a plain window is not; Sober's rule is the same effect, read in `the_rules_place_the_apps_they_name` |
| 630-633 | An idle pointer hides after 3 s over a Steam game, and only there | rig | `rules.sh` |
| 636 | YouTube Music to 8 | rig | `rules.sh` |
| 643 | Minecraft confines the pointer | rig, headless, unit | the rule `appId` `Minecraft.*` with `confinePointer`, as the config's class `^(Minecraft.*)$`: `cursor.sh` with a terminal named `Minecraft-rig`, headless with `Minecraft 1.21`; `Minecraft 1.21.4`, `Minecraft* 1.20.1` and `Minecraft` match and the launchers do not in `the_rules_place_the_apps_they_name`. The game itself, an X11 window: no, no Xwayland on the rig, needs the g815 |
| 657-660 | Video picture in picture: float, pin, 28%, bottom right | rig | `rules.sh`: `[721,537 287x215]`, there on every workspace |
| 664-667 | A helium popup that maps floating: the same | rig | `rules.sh` |
| 671-673 | The built-in notification: float, pin, top right | rig | `rules.sh`: `[648,16 360x120]` |
| 680-683 | `nd-hello` popups without blur | no | no Xwayland on the rig; the matcher in `tag_and_xwayland_matchers` |
| 688 | Atoll's tag floats | rig | `rules.sh`: `com.apple.TextEdit` tagged `atoll` floats at its own 420x260; untagged it tiles |
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
| Three outputs by description, mode, position, scale | headless, unit | headless `the_g815s_three_displays`: g815's host.json and its three displays named as their EDIDs name them (ShojiWM's `identity_is_spelled_as_libdisplay_info_spells_it` on the real blocks), HDMI-A-1 at 2560,0 2560x1440 at 1, DP-3 at 5120,440 1600x1000 at 1.6, eDP-1 at 6720,0 2048x1280 at 1.25, VRR entries 2, 0 and 3 and the core told VRR on the desk with any window fullscreen, on the panel only with a `steam_app_1` game, never on the portable, direct scanout off; an EDID that lands after its output connected names it for the config (ShojiWM `an_edid_read_after_the_output_connected_names_it_for_the_config`); stacks `1 2 3 5 6 7 9` and `4 8` showing 1 and 4; Super+4 and Super+8 from the desk focus DP-3, a window opened on 4 and a Discord window opened from the desk land inside DP-3, Super+Shift+8 takes a desk window there; the lid takes eDP-1 alone; `g815_lays_its_outputs_over_the_shared_file`, `a_hosts_outputs_come_before_the_shared_ones` |
| Workspaces 4 and 8 on the portable, the rest on the desk, 1 and 4 in view | headless, unit | two test outputs with the same pins |
| An output that goes takes its workspaces to the first, and gets them and its view back | rig, unit | `outputs.sh`: a made output disabled and enabled again, and removed; `an_output_that_returns_shows_what_it_showed_and_takes_its_workspaces_back` |
| The lid output | headless | as lines 95 to 114 above |
| `xwayland_primary` | no | no Xwayland on the rig |
| Opacity 0.96 | rig | `host.sh` reloaded onto the g815's file: 45 45 45 over white, where 0.96 of the terminal's 36 and 0.04 of 255 is 44.8 |
| `vrr = 2` on the desk: on while a window is fullscreen | rig, headless | `misc.sh`: asked of the core with a window fullscreen, not before and not after; headless the same on the second output |
| `vrr = 3` on the panel: on for a fullscreen game or video | rig, unit | `misc.sh` after a reload onto `vrr` 3: not for a plain window or a photo, asked for `steam_app_1` by its rule and for a client that declares video |
| A panel that does switch VRR on | no | the rig's connector is not `vrr_capable`; the core refuses with a warning and stays fixed |
| `direct_scanout = 0` | headless | as line 294 above |
| The environment | rig | `host.sh` |

## Motion

Each animation, the `hyprland.lua` leaf it replaces and the family it rides.
Read from the engine's frame log in `motion.sh`, and from `core/tests/scene.rs`.

| What moves | Leaf | Family | Evidence |
| --- | --- | --- | --- |
| A window's place and size in the tape: scroll, width, swap, stack, zen | `windows` | spatial | a scroll to x 10 goes on to 17 and comes back; `a_spatial_spring_overshoots_as_far_as_its_curve` |
| A window opening: from a point at its centre | `windows` | spatial | width from 5 px to 496 |
| Its opacity coming in | `fade` | effects | 0 to 100, no overshoot; `an_effects_spring_never_overshoots_and_arrives_on_time` |
| A window closing: shrinks to 90% | `windowsOut` | spatialFast | `90-closing` with the fast clocks slowed to two seconds |
| Its opacity going out | `fadeOut` | effectsFast | `90-closing` |
| The border colour | `border` | effectsSlow | 70 99 122 on the way, 67 133 190 landed |
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
| A tape scroll | a second scroll 50 ms in | continuous, largest step 104 px against 67 whole, over twice the way |
| A workspace switch | a second switch 50 ms in | continuous, largest step 156 px against 102 whole, over twice the way |
| The overview opening | closing 120 ms in | turns back at 93% and closes from there |
| Zen | zen off 50 ms in | the columns turn back from where they are drawn, largest step 136 px, the same as whole |
| A width change | the next preset 50 ms in | continuous, largest step 56 px |
| The console dropping in | park, then show 120 ms later | turns back at y -642, largest step 128 px against 102 whole |
| A window opening | its first commit | starts again from 5 px and opacity 0; until that commit the window has no buffer to draw |
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
| A tape scroll, four columns | 41 | 15.6, 25.3, 28.2 | 7.8, 11.7, 17.8 | 8.4, 12.0, 18.0 | 60 Hz |
| The overview opening | 40 | 18.6, 29.6, 34.0 | 9.6, 28.6, 32.4 | 10.0, 28.8, 32.5 | 43 Hz in the second it began |

The overview's worst interval is one frame of 34 ms at the open, two
refreshes, with a render of 32 ms in it; its median render is under one.

## The build

`formalwm/Cargo.toml` names `shojiwm_rs` and `shojiwm_lib` by a rev of the
fork's `formal` branch (7783d5e) and `Cargo.lock` is committed. `dev/fresh.sh` clones
this branch, copies only `wm/` and `crates/` to the rig and runs cargo in the
container with an empty target directory and an empty cargo home, no ShojiWM
checkout beside it:

| Command | Result |
| --- | --- |
| `cargo build --locked --profile release-fast -p formalwm` | exit 0 |
| `cargo test --locked --profile release-fast --workspace` | exit 0: 71, 4, 35, 11 and 5 in `formalwm-core`, 3 in `formalwm`, the headless session among them |
| `cargo clippy --locked --profile release-fast --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --locked -p fs-wm-proto` in `crates/` | 9 passed |

The clone was of e7c0b375.

## What the core does not offer

Each is read from the settings where the owner's config has it, and unused
until the core does.

- Opacity or animation of a layer surface.
- A window effect in the `behind` slot under a window drawn below opacity 1
  shows the window again, not the backdrop: a rule of 0.5 measured 0.75. The
  engine draws the blur as a node of the window's frame instead.
- The window's opacity kept off that blur: at opacity a, the sharp desk shows
  through at (1 - a) squared. 0.16% at the g815's 0.96.
- Xwayland's scale, a dialog for a client that stopped answering.
- A bound on the cursor's scale other than 8, and a pivot for it other than
  the hotspot.
- `bitdepth` and the SDR brightness and saturation of `outputs.set` `color`.

## Review and sweep findings

Each with the commit that settles it. `core` commits are on the fork's
`formal-rs`, each with a test there that failed before it (493 pass in
`shojiwm_rs` and `shojiwm_lib` at 9757796, 65 in `shoji_wm`; the fork's own
run of its workspace at e6493f8 has 581 passing).

| Finding | Commit | Check |
| --- | --- | --- |
| S1: an env name or value that cannot be set | 392f2c35 | `session.sh`; `an_env_name_or_value_that_cannot_be_set_is_refused` |
| S2: nothing in view to type into, the keyboard stays on a window | f119542c | `session2.sh`: a terminal sent away hears no more keys; headless |
| S3: on the lock screen a chord ran its unmarked binds too | 1a525ec4 | `binds.sh`; `on_the_lock_screen_a_chord_runs_only_its_binds_marked_locked` |
| S4: a zen or big window that changes workspace | 0a5e9e25 | `a_zen_or_big_window_that_changes_workspace_leaves_no_mark_behind` |
| S5: one window dragged out of a stack took its column along | d28af08b | `drags.sh`; `one_window_moved_out_of_a_stack_leaves_the_rest_of_its_column` |
| S6: an overlay shown on an output that goes | 7155a4af | `an_overlay_shown_on_an_output_that_goes_is_parked` |
| S7: a shortcut the core cannot read was dropped without a word | 6580404e | `session.sh`; `a_shortcut_the_core_cannot_read_is_refused` |
| B1: a window out of view handed the keyboard was shown for it | 67f7933e | headless; `a_window_is_in_view_on_a_shown_workspace_or_a_shown_overlay` |
| B2: what the layers reserve on each edge | 92dbe8db | `session2.sh`, `shell.sh`; `an_output_says_what_its_layers_reserve` |
| B3: `binds.list` without the shell's Escape | f0655471 | `session2.sh`; headless |
| The sidebar: the border ate into the gap | 4f27e28b, test 8cae4696 | the gap rows of spec 2 |
| `chrome.borderSize` | eedf7e5a | `session.sh`, `shell.sh`; `the_chrome_says_the_border_the_gaps_are_measured_to` |
| A made output's layers outlive it | core 2dc2846, 85f78ef | `shell.sh`: before 85f78ef every layer of one output was listed twice while there were two outputs; core tests `removing_an_output_closes_its_layers_and_moves_none`, `a_layer_is_stacked_once_with_two_outputs` |
| `outputs.set` `width`, `height` on a made output | d718d51d, core 4bd8be1 | `shell.sh`; core test `a_resized_headless_output_keeps_its_wl_output_and_layers` |
| Two release binds on one modifier merged | 4bb3ca8a, core 15b2700 | `binds.sh`; `each_bind_of_a_shortcut_keeps_what_it_says_of_itself`; core test `a_held_binding_tells_its_repeats_from_the_press` |
| A pointer lock outlived its window's focus | core a978879 | core tests `a_lock_follows_its_window_keyboard_focus`, `an_unfocused_confining_window_lets_the_pointer_go`; on the rig only the `confinePointer` side, in `cursor.sh`: no client there locks the pointer |
| `confinePointer` | 0fa49d27 | the rule row of spec 2 |
| The dynamic cursor | e4599d27, core 716706f | the cursor rows of spec 2; core test `a_stretch_along_a_turned_axis_leaves_the_image_upright` |
| S8: shake turned off while magnified left the cursor's clock running | 4cc164d2 | `cursor.sh`; `other_settings_drop_what_was_begun_under_the_last` |
| Black under everything, also after a reload | b103e542, 5c1ba125 | line 279 above |
| `directScanout`, auto HDR | 186f3cae, 2d0e1edf | lines 294 and 299 above |
| `binds.handOver` and the window's tag | 7513a855 | the hand-over and tag rows of spec 2 and 3 |
| The model and the seat drifted apart when an older window was focused over a newer one | 4ceb6e4f, core b5f290b | `focus.sh`; headless, which fails at the pick without the request and at the refused focus without the reconciling; core test `a_pick_is_not_refused_for_a_window_that_opened_later` |
| `bind.escape` took the Escape from the focused client | 9bf7fb45, core 8f0b97b | `focus.sh`; headless; core test `a_non_consuming_binding_runs_and_the_client_still_gets_the_event` |
| A bare modifier's `passthrough` release bind ran after a chord | 46842223 | `binds.sh`; headless; `a_bare_modifier_let_go_fires_after_the_modifier_alone`. The core had both kinds of release bind already (`tests/transparent_release.rs`) |
| The focused output did not follow the pointer | 3395202f, core 8f0b97b for the press | `outputs.sh`; headless; `the_pointer_takes_the_outputs_focus_and_leaves_the_keyboard` |
| S9: a workspace in view under the pointer, asked for, changed the focused window and left the keyboard on the other output | 279becde | `outputs.sh`; headless, which fails at the keyboard without it; `a_workspace_in_view_under_the_pointer_takes_the_keyboard_when_asked_for`. `workspace.focusAt`, the relative, occupied and last workspace go through the same line; a bind on the tape had the same skipped commit and has the same fix; `window.focus` and an overlay always commit, and `window.moveTo` over the socket does not follow |
| The release of a key a bind or a grab took reached the window focused when it came up (`key h up` with no `down` under the hand over) | core 0cf156b | core tests `tests/consumed_keys.rs`, four, each failing on e6493f8 with the stray release, and `key_pairing_tests`; on the build at 0cf156b the engine's tests and clippy, `overview.sh`, and `--mac-keys` with `FS_COMPOSITOR=shoji` ends `SMOKE_MAC_KEYS ok`: the stub logs `key q down` and `key q up` and no `h` or `l`, down or up, where the build at e6493f8 logs `key p up mods=68` and `key q up mods=64` with no `down` |
| A modifier whose key a bind or a grab took stayed down in the client (Shift pressed under a grab and let go after it); an input method's or a menu's keyboard grab heard the keys typed into the locker; keys held across a VT switch or on a keyboard unplugged stayed down in the client and their next press was dropped; a key two keyboards held came up when the first let go | core b62cbf7 | core tests `tests/consumed_keys.rs`: seven, each failing on 0cf156b, where the client ends shifted, the input method and the menu's client get the locker's `K`, Super stays down across the switch and the unplug, and A comes up while the other keyboard holds it; on the build at b62cbf7 the engine's tests and clippy, `focus.sh`, `overview.sh`, `--mac-keys` (formalwm and Hyprland) and `--login-lock` |
| A release the core made under the lock (a VT switch, an unplugged keyboard, a destroyed virtual keyboard) reached an input method's keyboard grab; the grab given back at unlock heard the release of a key pressed in the locker and kept the modifiers from before the lock; an input method's virtual keyboard typed into the locker; a repeating bind stopped when a second keyboard tapped its key | core 42a9a20 | core tests `tests/consumed_keys.rs`: seven, each failing on b62cbf7, where the input method gets `Up(37)` under the lock and `Up(K)` and `Down(30)` after the unlock, the locker gets the input method's virtual keyboard's keys and no `Up` for a key held across the unlock, and the bind's repeat is gone; three more pass on both: an input method grabbing under the lock hears nothing, a destroyed virtual keyboard lets go of its key, and an on-screen keyboard from another client still types into the locker. The input method's grab is dropped at the lock and not given back: fcitx5 and ibus grab again on activate. An input method's grab still learns of a modifier a bind took one key late, which needs Smithay to send modifiers before the key in `InputMethodKeyboardGrab`. On the build at 42a9a20 the engine's tests and clippy, `focus.sh`, `overview.sh`, `--mac-keys` (formalwm and Hyprland), `--login-lock`, `--login-switch` and `--login-stuck` |
| A client's virtual keyboard typed into the locker once it destroyed its input method; an input method's grab read the key after a modifier a bind or a grab took with the modifiers from before it; a menu opened after the unlock got no keyboard grab while the input method's grab object, dropped by the lock, still lived | core c9ae898 | core tests `tests/consumed_keys.rs`: three, each failing on 42a9a20, where the locker gets `[Mods(0), Mods(1), Down(1), Up(1), Mods(0)]` from the former input method, the input method gets `Down(A)` with no `Mods(1)` before it, and the keyboard is not grabbed after the menu opens; three more pass on both: a grab taken after the unlock hears no release of a key held in the locker, an input method's virtual keyboard on a second connection types into the locker (the core tells an input method by its connection, as the docs say), and a keymap sent under the lock reaches the locker only from an on-screen keyboard. The core sends the grab the seat's modifiers itself, on a serial of the seat's own as smithay does when the grab starts. On the build at c9ae898 the engine's tests and clippy, `focus.sh`, `overview.sh`, `--mac-keys` (formalwm and Hyprland), `--login-lock`, `--login-switch` and `--login-stuck` |
| `--mac-keys` stopped at SUPER + H under formalwm only. The model and the seat agreed: the settings float an `atoll` window (`example/settings.json`), and the column binds step over the tape, never onto a float, so SUPER + H from foot found no other column and nothing moved | the leg tiles the stub | `--mac-keys` under formalwm and Hyprland; `focus.sh`; core test `a_column_bind_leaves_a_float_for_the_tiles_and_never_steps_onto_one` |
| A lock surface that mapped under a still pointer got no pointer until it moved, so the shell's screensaver read the first move after the lock as an entry and dropped to the card (`--hotcorner-relock`); after the unlock the window under the still pointer stayed without it | core 61e4447 | core test `tests/lock_pointer_focus.rs`; `--hotcorner-relock` under formalwm and Hyprland; on the build at 50208b5 the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--lock`, `--unlock`, `--login-lock`, `--login-switch` and `--login-stuck` |
| The overview's input grab took the pointer off the shell's layers: the hot corner could not close it and the bar and minimap heard nothing while it was open | core 50208b5, the engine's grab asks for it | core test `tests/grab_layers.rs`: over a top layer surface the client has the pointer and its click, over a window the grab does, and a press the grab took stays the grab's over the bar; `--overview` under formalwm: a leave and a return at the corner close it, the minimap's press and drag out reach the shell; `overview.sh` |
| Under the lock the pointer fell through to the window, layer or popup under it where no lock surface was (not mapped yet, or a hole in its input region); a button held in a window when the lock went up kept its grab, a drag ran on and a locked pointer kept its relative motion; a window holding a button when the overview opened kept the pointer under it; a press the overview took came up in the window under the pointer after Escape; a click on an on-demand layer, or an exclusive layer mapping, took the keyboard from under the overview | core b5df06b | core tests `tests/pointer_isolation.rs`: seven, each failing on 50208b5 (the window gets `Motion`, `Down`, `Up` and `Scroll` under the lock, keeps its held button through the lock and the grab, the drag's source is not cancelled, the window gets `Up` after Escape, the layer gets the keyboard under the grab); on the build at b5df06b the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--login-lock`, `--login-switch`, `--login-stuck`, `--mac-keys`, `--hotcorner-relock`, `--overview`, `--dock` and `--mac-menu` under formalwm, `--mac-keys`, `--dock` and `--mac-menu` under Hyprland, and the shell's `dev/lint.sh` |
| Under the lock a window's client could take a menu grab, whose end handed it the keyboard and whose grab let it set the cursor, and start a drag on the locker's click; it could be activated through the config, and an on-demand layer mapped behind the lock took the keyboard at the unlock; a button held on the lock screen at the unlock came up in the window under it with no press; a button held across a VT switch came up in the window after the return | core c26b52c | core tests `tests/pointer_isolation.rs`: six failing on b5df06b, where the window gets `wl_keyboard.enter` under the lock and its cursor shape is set, the locker's click turns into the window's drag and is dropped, the config hears the activation, the layer gets the keyboard at the unlock, and the window gets `[Enter, Up]` for the locker's button and `Up` after the VT switch; five more pass on both: a destroyed lock surface, a second output with no lock surface, a pointer lock asked for under the lock, cursor requests under the lock, and a top layer's held button under the overview. On the build at c26b52c the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--login-lock`, `--login-switch`, `--login-stuck`, `--mac-keys`, `--hotcorner-relock`, `--overview`, `--dock` and `--mac-menu` under formalwm, `--mac-keys`, `--dock` and `--mac-menu` under Hyprland |
| Under the lock another client's input method got a fresh keyboard grab on request, with the keymap and the modifiers, so it could read Shift through the password; it could write into the locker's text input, and its candidate popup was tracked over the lock surface | core c036092 | core tests `tests/consumed_keys.rs`: three failing on c26b52c, where each fresh grab gets `repeat_info`, `keymap` and `modifiers`, the locker's text input gets `preedit_string`, `commit_string`, `delete_surrounding_text` and `done` from another client's input method, and the popup is tracked on the lock surface; two older tests now expect no `modifiers` at a grab under the lock. The core refuses the requests before smithay sees them: the grab object exists and hears nothing until the input method grabs again after the unlock, which the first test checks. On the build at c036092 the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--login-lock`, `--login-switch`, `--login-stuck`, `--mac-keys`, `--hotcorner-relock`, `--overview`, `--dock` and `--mac-menu` under formalwm, `--mac-keys`, `--dock` and `--mac-menu` under Hyprland |
| Under the lock another client's input method heard the locker's text input: `activate`, the content type, the surrounding text (all of a password field) and the cursor rectangle | core 88ac18e | core test `an_input_method_hears_nothing_of_the_lockers_text_input` in `tests/consumed_keys.rs`, failing on c036092, where the input method gets `activate`, `surrounding_text`, `text_change_cause` and `content_type` from the locker's password field; after the unlock it gets `deactivate` first and then the window's `activate` and surrounding text as before the lock. On the build at 88ac18e the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--login-lock`, `--login-switch`, `--login-stuck`, `--mac-keys`, `--hotcorner-relock`, `--overview`, `--dock` and `--mac-menu` under formalwm, `--mac-keys`, `--dock` and `--mac-menu` under Hyprland |
| Under the lock the locker's text input state still pending when another client replaced the seat's input method (destroying any input method clears smithay's) went to the replacement at the locker's next `commit`; the locker's own input method lost its keyboard grab at the first key; a grab refused under the lock still heard the modifiers | core 8218c2b | core tests in `tests/consumed_keys.rs`: `a_replacement_input_method_hears_nothing_the_locker_left_pending`, `the_lockers_input_method_keeps_its_keyboard_grab_under_the_lock` and `an_input_method_refused_a_grab_hears_no_modifiers`, failing on 88ac18e, where the replacement gets `activate`, `surrounding_text` and `content_type`, the locker's grab hears no key, and the refused grab gets `modifiers` while the locker's grab is live. `an_input_method_cannot_type_into_the_lockers_text_input` now activates the field with the locker's own input method and checks the `done` serials; it fails with the text guard removed. The text input's state is decided at `commit`, against the input method that would hear it, and is dropped across the lock and the unlock. On the build at 8218c2b the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--login-lock`, `--login-switch`, `--login-stuck`, `--mac-keys`, `--hotcorner-relock`, `--overview`, `--dock` and `--mac-menu` under formalwm, `--mac-keys`, `--dock` and `--mac-menu` under Hyprland |
| Under the lock an input method's keyboard grab from before the lock, released by its client, ended the locker's own grab: smithay's destructor clears the seat's one grab whichever grab object goes, so the rest of a key the locker's input method held went to the plain keyboard; a text input's state requests not yet committed were all kept, so a client sending them without `commit` grew the compositor's memory without bound | core 7783d5e | core tests `a_grab_from_before_the_lock_released_leaves_the_lockers_grab` in `tests/consumed_keys.rs` and `a_text_input_that_never_commits_holds_its_last_state_only` in `tests/text_input_pending.rs`, failing on 8218c2b, where the locker's grab hears `Down(K)` without its `Up(K)` and five thousand 2 kB `set_surrounding_text` requests grow the process by 12 MB. Only the destruction of the grab the keys go to, while it holds the keyboard, reaches smithay; a text input keeps the last request of each kind until its `commit`. On the build at 7783d5e the engine's tests and clippy, `focus.sh`, `overview.sh`, `session.sh`, `--login-lock`, `--login-switch`, `--login-stuck`, `--mac-keys`, `--hotcorner-relock`, `--overview`, `--dock` and `--mac-menu` under formalwm, `--mac-keys`, `--dock` and `--mac-menu` under Hyprland |

Needs the g815: Minecraft's confinement as an X11 window, the dynamic cursor
on a hardware cursor plane, the HDR switch on a fullscreen HDR window, what
`directScanout` does to a plane.
