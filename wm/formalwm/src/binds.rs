//! The settings' binds on the compositor's keys, and what each action does.

use formalwm_core::{
    action::{Action, Width, WorkspaceRef},
    chord::{Key, Mods},
    model::Big,
    settings::BindGroup,
};
use shojiwm_rs::prelude::*;

use crate::engine::{Engine, enter};

fn options_of(group: &BindGroup) -> KeyBindOptions {
    let mut options = KeyBindOptions::new();
    if group.release {
        options = options.on_release();
    }
    options.transparent = group.transparent;
    options.locked = group.locked;
    options.repeat = group.repeat;
    options
}

impl Engine {
    /// Registers every bind of the settings in place of the ones before.
    pub fn bind_keys(&mut self) {
        for id in self.bind_ids.drain(..) {
            COMPOSITOR.key.unbind(&id);
        }
        let settings = self.settings.clone();
        let mut drags = (None, None);
        for bind in &settings.binds {
            let modifier = || {
                let mods = bind.chord.mods;
                [
                    (mods.logo, "Super"),
                    (mods.alt, "Alt"),
                    (mods.ctrl, "Ctrl"),
                    (mods.shift, "Shift"),
                ]
                .into_iter()
                .find_map(|(held, name)| held.then_some(name))
            };
            // The core moves and resizes by a held modifier and the left
            // and right buttons.
            match (&bind.action, &bind.chord.key) {
                (Action::MoveWindow, Key::Button(_)) => drags.0 = modifier(),
                (Action::ResizeWindow, Key::Button(_)) => drags.1 = modifier(),
                _ => {}
            }
        }
        for (index, group) in settings.bind_groups().into_iter().enumerate() {
            let id = format!("formalwm:{index}");
            let shortcut = group.shortcut.clone();
            let options = options_of(&group);
            // The core fires a shortcut marked `locked` in both states; which
            // of its binds run is theirs to say.
            let handler = move || {
                let locked = COMPOSITOR.session.is_locked();
                let actions: Vec<Action> = group.actions(locked).cloned().collect();
                enter(move |engine| {
                    for action in &actions {
                        engine.run(action);
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

    /// Runs the binds of a chord the engine was given instead of the core:
    /// under a grab no bind of the core's fires.
    pub fn run_chord(&mut self, key: &str, code: u32, mods: Mods) {
        let locked = COMPOSITOR.session.is_locked();
        let actions: Vec<Action> = self
            .settings
            .binds
            .iter()
            .filter(|bind| !bind.release && bind.chord.mods == mods)
            .filter(|bind| bind.locked || !locked)
            .filter(|bind| match &bind.chord.key {
                Key::Sym(name) => name.eq_ignore_ascii_case(key),
                Key::Code(bound) => *bound == code,
                _ => false,
            })
            .map(|bind| bind.action.clone())
            .collect();
        for action in actions {
            self.run(&action);
        }
    }

    /// The same for a modifier let go under a grab.
    pub fn run_release(&mut self, key: &str) {
        let mut actions: Vec<Action> = Vec::new();
        let locked = COMPOSITOR.session.is_locked();
        let binds = self.settings.binds.iter();
        for bind in binds.filter(|bind| bind.release && (bind.locked || !locked)) {
            let released =
                matches!(&bind.chord.key, Key::Sym(name) if name.eq_ignore_ascii_case(key));
            if released && !actions.contains(&bind.action) {
                actions.push(bind.action.clone());
            }
        }
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
