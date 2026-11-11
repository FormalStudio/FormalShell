//! The settings' binds on the compositor's keys, and what each action does.

use std::collections::BTreeMap;

use formalwm_core::{
    action::{Action, Width, WorkspaceRef},
    chord::{Key, Mods},
    model::Big,
};
use shojiwm_rs::prelude::*;

use crate::engine::{Engine, enter};

impl Engine {
    /// Registers every bind of the settings in place of the ones before.
    /// Hyprland runs every bind a chord has; ShojiWM fires one shortcut
    /// once, so the binds of one chord share a registration.
    pub fn bind_keys(&mut self) {
        for id in self.bind_ids.drain(..) {
            COMPOSITOR.key.unbind(&id);
        }
        if let Some(grab) = self.release_grab.take() {
            grab.release();
        }
        let settings = self.settings.clone();
        self.releases.clear();
        for bind in settings.binds.iter().filter(|bind| bind.release) {
            if let Some(modifier) = bind.chord.modifier_key() {
                let actions = self.releases.entry(modifier).or_default();
                if !actions.contains(&bind.action) {
                    actions.push(bind.action.clone());
                }
            }
        }
        type Group = (KeyBindOptions, Vec<Action>, Option<&'static str>);
        let mut groups: BTreeMap<(String, bool), Group> = BTreeMap::new();
        let mut drags = (None, None);
        for bind in &settings.binds {
            let held = || {
                let mods = bind.chord.mods;
                [
                    (mods.logo, "Super"),
                    (mods.alt, "Alt"),
                    (mods.ctrl, "Ctrl"),
                    (mods.shift, "Shift"),
                ]
                .into_iter()
                .filter(|(held, _)| *held)
                .map(|(_, name)| name)
            };
            let modifier = || held().next();
            match (&bind.action, &bind.chord.key) {
                // The core moves and resizes by a held modifier and the
                // left and right buttons.
                (Action::MoveWindow, Key::Button(_)) => drags.0 = modifier(),
                (Action::ResizeWindow, Key::Button(_)) => drags.1 = modifier(),
                _ => {
                    let shortcut = match (bind.release, bind.chord.modifier_key()) {
                        // Letting go of a modifier: the core names the
                        // modifier alone.
                        (true, Some(modifier)) => Some(modifier.to_owned()),
                        _ => bind.chord.shortcut(),
                    };
                    match shortcut {
                        Some(shortcut) => {
                            // Both Alt keys let go are one shortcut here.
                            let (options, actions, waits) =
                                groups.entry((shortcut, bind.release)).or_default();
                            if bind.release {
                                *options = options.on_release();
                            } else if waits.is_none() {
                                *waits = held().find(|name| self.releases.contains_key(name));
                            }
                            options.locked |= bind.locked;
                            options.repeat |= bind.repeat && !bind.release;
                            if !actions.contains(&bind.action) {
                                actions.push(bind.action.clone());
                            }
                        }
                        None => tracing::warn!(
                            chord = bind.chord.canonical(),
                            "a pointer button binds only move-window and resize-window"
                        ),
                    }
                }
            }
        }
        for (index, ((shortcut, _), (options, actions, waits))) in groups.into_iter().enumerate() {
            let id = format!("formalwm:{index}");
            let handler = move || {
                let actions = actions.clone();
                enter(move |engine| {
                    for action in &actions {
                        engine.run(action);
                    }
                    if let Some(modifier) = waits {
                        engine.await_release(modifier);
                    }
                })
            };
            COMPOSITOR.key.bind_with(&id, &shortcut, options, handler);
            self.bind_ids.push(id);
        }
        if let Some(modifier) = drags.0 {
            COMPOSITOR.pointer.bind_window_move_modifier(modifier);
        }
        if let Some(modifier) = drags.1 {
            COMPOSITOR.pointer.bind_window_resize_modifier(modifier);
        }
        self.bind_escapes();
    }

    /// The core fires a bind on a modifier's release only when the modifier
    /// was tapped alone. Letting it go after a chord, which is how Alt+Tab
    /// commits, is watched here: from a bind on that modifier until it is
    /// let go the engine has the keys and runs their binds itself.
    fn await_release(&mut self, modifier: &'static str) {
        let grabbing = |grab: &Option<InputGrab>| grab.as_ref().is_some_and(InputGrab::is_active);
        if grabbing(&self.release_grab) || grabbing(&self.overview.grab) {
            return;
        }
        let options = InputGrabOptions::new()
            .on_key(move |event| {
                let key = event.key.clone();
                let pressed = event.state == InputGrabState::Pressed;
                let mods = Mods {
                    logo: event.modifiers.logo,
                    shift: event.modifiers.shift,
                    ctrl: event.modifiers.ctrl,
                    alt: event.modifiers.alt,
                };
                enter(move |engine| engine.release_key(modifier, &key, mods, pressed));
            })
            .on_cancel(|_| enter(|engine| engine.release_grab = None));
        self.release_grab = Some(COMPOSITOR.input.grab(options));
    }

    fn release_key(&mut self, modifier: &str, key: &str, mods: Mods, pressed: bool) {
        if pressed {
            return self.run_chord(key, mods);
        }
        let let_go = matches!(
            (modifier, key),
            ("Alt", "Alt_L" | "Alt_R")
                | ("Super", "Super_L" | "Super_R")
                | ("Shift", "Shift_L" | "Shift_R")
                | ("Ctrl", "Control_L" | "Control_R")
        );
        if !let_go {
            return;
        }
        if let Some(grab) = self.release_grab.take() {
            grab.release();
        }
        for action in self.releases.get(modifier).cloned().unwrap_or_default() {
            self.run(&action);
        }
    }

    /// Runs the binds of a chord the engine was given instead of the core:
    /// under a grab no bind of the core's fires.
    pub fn run_chord(&mut self, key: &str, mods: Mods) {
        let actions: Vec<Action> = self
            .settings
            .binds
            .iter()
            .filter(|bind| !bind.release && bind.chord.mods == mods)
            .filter(
                |bind| matches!(&bind.chord.key, Key::Sym(name) if name.eq_ignore_ascii_case(key)),
            )
            .map(|bind| bind.action.clone())
            .collect();
        for action in actions {
            self.run(&action);
        }
    }

    /// `bind.escape`: while the shell has something open, a plain Escape
    /// runs its command.
    pub fn bind_escapes(&mut self) {
        const ID: &str = "formalwm:escape";
        COMPOSITOR.key.unbind(ID);
        if self.escapes.is_empty() {
            return;
        }
        COMPOSITOR.key.bind(ID, "Escape", || {
            enter(|engine| {
                for argv in engine.escapes.values() {
                    COMPOSITOR.process.spawn(Command::exec(argv.clone()));
                }
            })
        });
    }

    pub fn run(&mut self, action: &Action) {
        let focused = self.model.focused_window().cloned();
        let changed = match action {
            Action::Spawn(argv) => {
                COMPOSITOR.process.spawn(Command::exec(argv.clone()));
                false
            }
            Action::Shell(line) => {
                COMPOSITOR.process.spawn(Command::shell(line.clone()));
                false
            }
            Action::Terminal => {
                COMPOSITOR
                    .process
                    .spawn(Command::shell(self.settings.terminal.clone()));
                false
            }
            Action::Close => {
                if let Some(win) = focused.and_then(|id| self.windows.get(&id)) {
                    win.handle.close();
                }
                false
            }
            Action::Fullscreen => {
                if let Some(id) = focused {
                    self.toggle_fullscreen(&id);
                }
                false
            }
            Action::Maximize => {
                focused.is_some_and(|id| self.model.toggle_big(&id, Big::Maximized))
            }
            Action::Float => focused.is_some_and(|id| self.set_floating(&id, None)),
            Action::FocusColumn(side) => self.model.focus_column(*side),
            Action::FocusWindow(side) => self.model.focus_row(*side),
            Action::SwapColumn(side) => self.model.swap_column(*side),
            Action::SwapWindow(side) => self.model.swap_row(*side),
            Action::ColumnWidth(Width::Set(width)) => self.model.set_column_width(*width),
            Action::ColumnWidth(Width::Add(step)) => self.model.resize_column(*step),
            Action::ColumnWidth(Width::Cycle) => self.model.cycle_column_width(),
            Action::Fit => self.model.fit_into_view(),
            Action::ConsumeOrExpel(side) => self.model.consume_or_expel(*side),
            Action::Workspace(WorkspaceRef::Number(number)) => {
                self.model.focus_workspace(&number.to_string())
            }
            Action::Workspace(WorkspaceRef::Previous) => self.model.focus_previous_workspace(),
            Action::Workspace(WorkspaceRef::Relative(step)) => {
                self.model.focus_workspace_relative(*step)
            }
            Action::Workspace(WorkspaceRef::Occupied(step)) => {
                self.model.focus_occupied_workspace(*step)
            }
            Action::MoveToWorkspace(number) => focused
                .is_some_and(|id| self.model.move_to_workspace(&id, &number.to_string(), true)),
            Action::Overview(open) => {
                self.set_overview(*open);
                false
            }
            Action::Zen => self.model.set_zen(None, None),
            Action::MoveWindow | Action::ResizeWindow => false,
            Action::Exit => {
                crate::session::exit();
                false
            }
        };
        if changed {
            self.commit();
        }
    }

    /// The client is asked; the model follows when the compositor reports
    /// the request, as it does for one the client makes itself.
    pub fn toggle_fullscreen(&mut self, id: &str) {
        let Some(win) = self.windows.get(id) else {
            return;
        };
        if self.model.big_of(id) == Some(Big::Fullscreen) {
            win.handle.unfullscreen();
        } else {
            win.handle.fullscreen();
        }
    }

    /// `floating` absent toggles. A float is told its rect's size from
    /// here on, whatever it was allowed to choose when it opened.
    pub fn set_floating(&mut self, id: &str, floating: Option<bool>) -> bool {
        if !self.model.set_floating(id, floating) {
            return false;
        }
        if let Some(win) = self.windows.get(id) {
            win.handle.state(&crate::windows::FORCE).set(true);
        }
        true
    }
}
