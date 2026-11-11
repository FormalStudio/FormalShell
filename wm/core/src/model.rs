//! What the engine knows: outputs, their stacks of workspaces, each
//! workspace's tape and floats, the overlays, and who has the focus. Every
//! operation of the binds and the socket is a method here; nothing in this
//! file touches the compositor.

use std::collections::BTreeMap;

use crate::{
    geom::Rect,
    tape::{Side, Tape, WinId},
};

pub type WsId = String;

/// The shell's `space.md`, the band the zen window keeps from the output's
/// edge (`wayland/zen.rs`).
const ZEN_MARGIN: f64 = 12.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub default_width: f64,
    pub presets: Vec<f64>,
    /// Per window side.
    pub gaps_in: f64,
    /// At the output's edge.
    pub gaps_out: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            default_width: 0.5,
            presets: vec![0.333, 0.5, 0.667, 1.0],
            gaps_in: 4.0,
            gaps_out: 8.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSpec {
    pub name: String,
    /// The output it lives on when that output is connected.
    pub pin: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputGeo {
    pub name: String,
    /// Global logical.
    pub rect: Rect,
    /// Global logical, what the layers leave free.
    pub usable: Rect,
    pub scale: f64,
    /// The id of the workspace it shows first.
    pub default_workspace: Option<WsId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub geo: OutputGeo,
    pub active: WsId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Big {
    /// To the edges of what the layers leave free.
    Maximized,
    /// The whole output, no chrome.
    Fullscreen,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub id: WsId,
    pub name: String,
    pub output: String,
    pub pin: Option<String>,
    /// The output it was on when that one went; it goes back there.
    pub from: Option<String>,
    /// From the settings. One opened in the overview is not, and goes when
    /// it is empty and off screen.
    pub named: bool,
    pub tape: Tape,
    /// Bottom to top.
    pub floats: Vec<WinId>,
    /// The top float has the focus, not the tape.
    pub float_focused: bool,
    pub big: Option<(WinId, Big)>,
    pub zen: Option<WinId>,
}

impl Workspace {
    fn new(id: WsId, name: String, pin: Option<String>, named: bool) -> Self {
        Self {
            id,
            name,
            output: String::new(),
            pin,
            from: None,
            named,
            tape: Tape::default(),
            floats: Vec::new(),
            float_focused: false,
            big: None,
            zen: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tape.is_empty() && self.floats.is_empty()
    }

    pub fn focused(&self) -> Option<&WinId> {
        if let Some(zen) = &self.zen {
            return Some(zen);
        }
        if self.float_focused
            && let Some(top) = self.floats.last()
        {
            return Some(top);
        }
        self.tape.focused().or(self.floats.last())
    }

    pub fn windows(&self) -> impl Iterator<Item = &WinId> {
        self.tape.windows().chain(self.floats.iter())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub name: String,
    /// Bottom to top.
    pub windows: Vec<WinId>,
    pub shown_on: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Home {
    Workspace(WsId),
    Overlay(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowRec {
    pub id: WinId,
    pub title: String,
    pub app_id: String,
    pub initial_title: String,
    pub initial_app_id: String,
    pub pid: i64,
    pub home: Home,
    pub floating: bool,
    /// Relative to its output's origin; kept while tiled for the next float.
    pub float_rect: Rect,
    /// Shown on every workspace of its output.
    pub pinned: bool,
    pub urgent: bool,
}

/// Where a new window goes, the window rules already applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    pub workspace: Option<WsId>,
    pub overlay: Option<String>,
    pub floating: bool,
    /// Relative to the output's origin.
    pub float_rect: Option<Rect>,
    pub pinned: bool,
    pub fullscreen: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewWindow {
    pub id: WinId,
    pub title: String,
    pub app_id: String,
    pub pid: i64,
    /// The size it asked for, for a float with no rule about its size.
    pub size: Option<(f64, f64)>,
}

/// Stacking classes, bottom to top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Tile,
    Maximized,
    Zen,
    Float,
    Pinned,
    Fullscreen,
    Overlay,
}

/// One window's place at rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: WinId,
    pub output: String,
    /// None on an overlay.
    pub workspace: Option<WsId>,
    /// Global logical, as if its workspace were the active one, the tape's
    /// scroll left out for a window that `scrolls`.
    pub rect: Rect,
    pub layer: Layer,
    /// Order inside the layer, bottom to top.
    pub order: usize,
    pub scrolls: bool,
    /// -1 when not in a column.
    pub column: i32,
    pub row: i32,
    /// On screen at rest: on its output's active workspace, or on a shown
    /// overlay.
    pub shown: bool,
}

/// A tiled window lifted out of the tape by a drag.
#[derive(Clone, Debug, PartialEq)]
pub struct Lifted {
    pub id: WinId,
    pub workspace: WsId,
    pub column: usize,
    pub row: usize,
    pub width: f64,
    /// It had the column to itself.
    pub alone: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub options: Options,
    pub outputs: Vec<Output>,
    /// In stack order: an output's stack is its workspaces in this order.
    pub workspaces: Vec<Workspace>,
    /// In first-seen order.
    pub windows: Vec<WindowRec>,
    pub overlays: Vec<Overlay>,
    pub focused_output: String,
    /// Where `workspace previous` goes.
    pub previous_workspace: Option<WsId>,
    /// What an output that went was showing, for when it returns.
    shown: BTreeMap<String, WsId>,
    next_dynamic: u32,
}

impl Model {
    pub fn new(options: Options, specs: &[WorkspaceSpec]) -> Self {
        let mut model = Self {
            options,
            ..Self::default()
        };
        model.set_workspaces(specs);
        model
    }

    // Settings and outputs.

    /// Applies the settings' workspaces by position. Windows stay where they
    /// are: a workspace the settings no longer name lives on unnamed until
    /// it empties.
    pub fn set_workspaces(&mut self, specs: &[WorkspaceSpec]) {
        for (index, spec) in specs.iter().enumerate() {
            let id = (index + 1).to_string();
            match self.workspaces.iter_mut().find(|ws| ws.id == id) {
                Some(ws) => {
                    ws.name = spec.name.clone();
                    ws.pin = spec.pin.clone();
                    ws.named = true;
                }
                None => {
                    let at = self
                        .workspaces
                        .iter()
                        .rposition(|ws| ws.named)
                        .map_or(0, |last| last + 1);
                    self.workspaces.insert(
                        at,
                        Workspace::new(id, spec.name.clone(), spec.pin.clone(), true),
                    );
                }
            }
        }
        for ws in &mut self.workspaces {
            if ws.named && ws.id.parse::<usize>().is_ok_and(|n| n > specs.len()) {
                ws.named = false;
                ws.pin = None;
            }
        }
        let outputs = self.outputs.iter().map(|o| o.geo.clone()).collect();
        self.set_outputs(outputs);
    }

    /// The connected, enabled outputs. Each workspace goes to the output it
    /// is pinned to, back to the one it left, stays where it is, or falls
    /// to the first output.
    pub fn set_outputs(&mut self, outputs: Vec<OutputGeo>) {
        let has = |name: &str| outputs.iter().any(|o| o.name == name);
        let first = outputs.first().map(|o| o.name.clone()).unwrap_or_default();
        for ws in &mut self.workspaces {
            let back = ws.from.clone().filter(|name| has(name));
            ws.output = match (&ws.pin, back) {
                (Some(pin), _) if has(pin) => pin.clone(),
                (_, Some(back)) => back,
                _ if has(&ws.output) => ws.output.clone(),
                _ => {
                    if ws.from.is_none() && !ws.output.is_empty() {
                        ws.from = Some(ws.output.clone());
                    }
                    first.clone()
                }
            };
            if ws.from.as_deref() == Some(ws.output.as_str()) {
                ws.from = None;
            }
        }
        let old = std::mem::take(&mut self.outputs);
        for output in old.iter().filter(|o| !has(&o.geo.name)) {
            self.shown
                .insert(output.geo.name.clone(), output.active.clone());
        }
        for geo in outputs {
            let on_output = |id: &str| {
                self.workspaces
                    .iter()
                    .any(|ws| ws.id == id && ws.output == geo.name)
            };
            let kept = old
                .iter()
                .find(|o| o.geo.name == geo.name)
                .map(|o| o.active.clone())
                .or_else(|| self.shown.remove(&geo.name))
                .filter(|id| on_output(id));
            let active = kept
                .or_else(|| geo.default_workspace.clone().filter(|id| on_output(id)))
                .or_else(|| {
                    self.workspaces
                        .iter()
                        .find(|ws| ws.output == geo.name)
                        .map(|ws| ws.id.clone())
                })
                .unwrap_or_else(|| self.add_workspace_for(&geo.name));
            self.outputs.push(Output { geo, active });
        }
        if !self
            .outputs
            .iter()
            .any(|o| o.geo.name == self.focused_output)
        {
            self.focused_output = first;
        }
        for index in 0..self.workspaces.len() {
            let strip_w = self.strip_of(&self.workspaces[index].output).w;
            self.workspaces[index].tape.clamp_scroll(strip_w);
        }
        for id in self
            .outputs
            .iter()
            .map(|o| o.active.clone())
            .collect::<Vec<_>>()
        {
            self.adopt_pinned(&id);
        }
        self.collect();
    }

    pub fn set_options(&mut self, options: Options) {
        self.options = options;
        for index in 0..self.workspaces.len() {
            let strip_w = self.strip_of(&self.workspaces[index].output).w;
            self.workspaces[index].tape.fit(strip_w);
        }
    }

    fn add_workspace_for(&mut self, output: &str) -> WsId {
        self.next_dynamic += 1;
        let id = format!("d{}", self.next_dynamic);
        let mut ws = Workspace::new(id.clone(), String::new(), None, false);
        ws.output = output.to_owned();
        self.workspaces.push(ws);
        id
    }

    // Lookups.

    pub fn output(&self, name: &str) -> Option<&Output> {
        self.outputs.iter().find(|o| o.geo.name == name)
    }

    fn output_mut(&mut self, name: &str) -> Option<&mut Output> {
        self.outputs.iter_mut().find(|o| o.geo.name == name)
    }

    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id == id)
    }

    fn workspace_mut(&mut self, id: &str) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|ws| ws.id == id)
    }

    pub fn window(&self, id: &str) -> Option<&WindowRec> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: &str) -> Option<&mut WindowRec> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// An output's workspaces, top to bottom.
    pub fn stack(&self, output: &str) -> Vec<&Workspace> {
        self.workspaces
            .iter()
            .filter(|ws| ws.output == output)
            .collect()
    }

    /// A workspace's place in its output's stack, from 0.
    pub fn stack_index(&self, id: &str) -> Option<usize> {
        let ws = self.workspace(id)?;
        self.stack(&ws.output).iter().position(|w| w.id == id)
    }

    pub fn active_workspace(&self, output: &str) -> Option<&Workspace> {
        self.workspace(&self.output(output)?.active)
    }

    pub fn focused_workspace(&self) -> Option<&Workspace> {
        self.active_workspace(&self.focused_output)
    }

    fn focused_workspace_mut(&mut self) -> Option<&mut Workspace> {
        let id = self.output(&self.focused_output)?.active.clone();
        self.workspace_mut(&id)
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.outputs.iter().any(|o| o.active == id)
    }

    pub fn shown_overlay(&self, output: &str) -> Option<&Overlay> {
        self.overlays.iter().find(|overlay| {
            overlay.shown_on.as_deref() == Some(output) && !overlay.windows.is_empty()
        })
    }

    /// The window the keyboard belongs to.
    pub fn focused_window(&self) -> Option<&WinId> {
        if let Some(overlay) = self.shown_overlay(&self.focused_output) {
            return overlay.windows.last();
        }
        self.focused_workspace()?.focused()
    }

    /// The tape's strip on an output: what the layers leave free, inset so a
    /// window's own gap adds up to the outer gap at the edge.
    pub fn strip_of(&self, output: &str) -> Rect {
        self.output(output)
            .map(|o| {
                o.geo
                    .usable
                    .inset(self.options.gaps_out - self.options.gaps_in)
            })
            .unwrap_or_default()
    }

    fn strip_w(&self, ws: &str) -> f64 {
        self.workspace(ws)
            .map(|ws| self.strip_of(&ws.output).w)
            .unwrap_or_default()
    }

    /// The zen window's rect on an output, the shell's `centred`.
    pub fn zen_rect(&self, output: &str) -> Rect {
        let Some(output) = self.output(output) else {
            return Rect::default();
        };
        let Rect { x, y, w, h } = output.geo.rect;
        let width = (w * 0.6).max((w - 2.0 * ZEN_MARGIN).min(1100.0));
        let height = h - 2.0 * (h * 0.06).max(ZEN_MARGIN);
        Rect::new(
            x + ((w - width) / 2.0).round(),
            y + ((h - height) / 2.0).round(),
            width.round(),
            height.round(),
        )
    }

    // Windows.

    /// Adds a window where its placement says and gives it the focus. A
    /// window sent to a workspace takes the view there with it.
    pub fn open(&mut self, new: NewWindow, placement: Placement) {
        if self.window(&new.id).is_some() {
            return;
        }
        let overlay = placement.overlay.clone();
        let workspace = placement
            .workspace
            .clone()
            .filter(|id| self.workspace(id).is_some())
            .or_else(|| self.focused_workspace().map(|ws| ws.id.clone()))
            .unwrap_or_default();
        let output = self
            .workspace(&workspace)
            .map(|ws| ws.output.clone())
            .unwrap_or_default();
        let floating = placement.floating || overlay.is_some();
        let float_rect = placement
            .float_rect
            .unwrap_or_else(|| self.default_float_rect(&output, new.size));
        self.windows.push(WindowRec {
            id: new.id.clone(),
            initial_title: new.title.clone(),
            initial_app_id: new.app_id.clone(),
            title: new.title,
            app_id: new.app_id,
            pid: new.pid,
            home: match &overlay {
                Some(name) => Home::Overlay(name.clone()),
                None => Home::Workspace(workspace.clone()),
            },
            floating,
            float_rect,
            pinned: placement.pinned && floating,
            urgent: false,
        });
        if let Some(name) = overlay {
            self.overlay_mut(&name).windows.push(new.id);
            return;
        }
        self.attach(&new.id, &workspace, None);
        if placement.fullscreen {
            self.set_big(&new.id, Some(Big::Fullscreen));
        }
        self.show_workspace(&workspace);
    }

    fn default_float_rect(&self, output: &str, size: Option<(f64, f64)>) -> Rect {
        let area = self
            .output(output)
            .map(|o| Rect::new(0.0, 0.0, o.geo.rect.w, o.geo.rect.h))
            .unwrap_or(Rect::new(0.0, 0.0, 1280.0, 720.0));
        let (w, h) = size
            .filter(|(w, h)| *w > 0.0 && *h > 0.0)
            .unwrap_or((area.w * 0.5, area.h * 0.6));
        let (w, h) = (w.min(area.w), h.min(area.h));
        Rect::new(
            ((area.w - w) / 2.0).round(),
            ((area.h - h) / 2.0).round(),
            w.round(),
            h.round(),
        )
    }

    /// Puts a window the model already knows on a workspace, as a new column
    /// (at `column`, or right of the focus) or as its top float.
    fn attach(&mut self, id: &str, workspace: &str, column: Option<usize>) {
        let Some(floating) = self.window(id).map(|w| w.floating) else {
            return;
        };
        let width = self.options.default_width;
        let strip_w = self.strip_w(workspace);
        let Some(ws) = self.workspace_mut(workspace) else {
            return;
        };
        if floating {
            ws.floats.push(id.to_owned());
            ws.float_focused = true;
        } else {
            match column {
                Some(column) => ws.tape.insert_at(column, id.to_owned(), width),
                None => ws.tape.insert(id.to_owned(), width),
            }
            ws.tape.fit(strip_w);
            ws.float_focused = false;
            ws.big = None;
        }
        if let Some(window) = self.window_mut(id) {
            window.home = Home::Workspace(workspace.to_owned());
        }
    }

    /// Takes a window out of its workspace or overlay, leaving its record.
    fn detach(&mut self, id: &str) {
        let Some(home) = self.window(id).map(|w| w.home.clone()) else {
            return;
        };
        match home {
            Home::Overlay(name) => {
                let overlay = self.overlay_mut(&name);
                overlay.windows.retain(|w| w != id);
                if overlay.windows.is_empty() {
                    overlay.shown_on = None;
                }
            }
            Home::Workspace(workspace) => {
                let strip_w = self.strip_w(&workspace);
                if let Some(ws) = self.workspace_mut(&workspace) {
                    ws.tape.remove(id);
                    ws.tape.fit(strip_w);
                    ws.floats.retain(|w| w != id);
                    if ws.floats.is_empty() {
                        ws.float_focused = false;
                    }
                    if ws.big.as_ref().is_some_and(|(big, _)| big == id) {
                        ws.big = None;
                    }
                    if ws.zen.as_deref() == Some(id) {
                        ws.zen = None;
                    }
                }
            }
        }
    }

    pub fn close(&mut self, id: &str) {
        self.detach(id);
        self.windows.retain(|w| w.id != id);
        self.collect();
    }

    /// Drops unnamed workspaces that are empty and not on screen.
    fn collect(&mut self) {
        let active: Vec<WsId> = self.outputs.iter().map(|o| o.active.clone()).collect();
        self.workspaces
            .retain(|ws| ws.named || !ws.is_empty() || active.contains(&ws.id));
    }

    /// Makes a workspace the one its output shows and gives that output the
    /// focus.
    fn show_workspace(&mut self, id: &str) {
        let Some(output) = self.workspace(id).map(|ws| ws.output.clone()) else {
            return;
        };
        let current = self.focused_workspace().map(|ws| ws.id.clone());
        if current.as_deref() != Some(id) {
            self.previous_workspace = current;
        }
        if let Some(output) = self.output_mut(&output) {
            output.active = id.to_owned();
        }
        self.focused_output = output;
        self.adopt_pinned(id);
        self.collect();
    }

    /// Pinned floats follow their output to the workspace it shows.
    fn adopt_pinned(&mut self, id: &str) {
        let Some(output) = self.workspace(id).map(|ws| ws.output.clone()) else {
            return;
        };
        let pinned: Vec<WinId> = self
            .windows
            .iter()
            .filter(|w| w.pinned && w.floating)
            .filter(|w| match &w.home {
                Home::Workspace(home) => {
                    home != id && self.workspace(home).is_some_and(|ws| ws.output == output)
                }
                Home::Overlay(_) => false,
            })
            .map(|w| w.id.clone())
            .collect();
        for window in pinned {
            let focused = self.workspace(id).map(|ws| ws.float_focused);
            self.detach(&window);
            self.attach(&window, id, None);
            if let (Some(ws), Some(focused)) = (self.workspace_mut(id), focused) {
                ws.float_focused = focused;
            }
        }
    }

    /// Gives a window the focus, going to its workspace. A window on a
    /// parked overlay stays parked.
    pub fn focus_window(&mut self, id: &str) -> bool {
        let Some(window) = self.window(id) else {
            return false;
        };
        match window.home.clone() {
            Home::Overlay(name) => {
                let overlay = self.overlay_mut(&name);
                let Some(output) = overlay.shown_on.clone() else {
                    return false;
                };
                raise(&mut overlay.windows, id);
                self.focused_output = output;
            }
            Home::Workspace(workspace) => {
                let floating = window.floating;
                let strip_w = self.strip_w(&workspace);
                let Some(ws) = self.workspace_mut(&workspace) else {
                    return false;
                };
                if ws.zen.as_deref().is_some_and(|zen| zen != id) {
                    ws.zen = None;
                }
                if floating {
                    raise(&mut ws.floats, id);
                    ws.float_focused = true;
                } else {
                    ws.tape.focus_window(id);
                    ws.tape.fit(strip_w);
                    ws.float_focused = false;
                    if ws.big.as_ref().is_some_and(|(big, _)| big != id) {
                        ws.big = None;
                    }
                }
                let output = ws.output.clone();
                if self
                    .overlays
                    .iter()
                    .any(|o| o.shown_on.as_deref() == Some(&output))
                {
                    self.hide_overlays_on(&output);
                }
                self.show_workspace(&workspace);
            }
        }
        true
    }

    pub fn set_title(&mut self, id: &str, title: &str) {
        if let Some(window) = self.window_mut(id) {
            window.title = title.to_owned();
        }
    }

    pub fn set_app_id(&mut self, id: &str, app_id: &str) {
        if let Some(window) = self.window_mut(id) {
            window.app_id = app_id.to_owned();
        }
    }

    pub fn set_urgent(&mut self, id: &str, urgent: bool) {
        let focused = self.focused_window().is_some_and(|f| f == id);
        if let Some(window) = self.window_mut(id) {
            window.urgent = urgent && !focused;
        }
    }

    /// `floating` absent toggles. A window back in the tape opens a column
    /// right of the focus; one leaving it takes the float rect it last had.
    pub fn set_floating(&mut self, id: &str, floating: Option<bool>) -> bool {
        let Some(window) = self.window(id) else {
            return false;
        };
        let Home::Workspace(workspace) = window.home.clone() else {
            return false;
        };
        let floating = floating.unwrap_or(!window.floating);
        if floating == window.floating {
            return false;
        }
        self.detach(id);
        if let Some(window) = self.window_mut(id) {
            window.floating = floating;
            window.pinned &= floating;
        }
        self.attach(id, &workspace, None);
        true
    }

    /// Pins or unpins a floating window.
    pub fn set_pinned(&mut self, id: &str, pinned: bool) {
        if let Some(window) = self.window_mut(id) {
            window.pinned = pinned && window.floating;
        }
    }

    pub fn set_big(&mut self, id: &str, big: Option<Big>) -> bool {
        let Some(Home::Workspace(workspace)) = self.window(id).map(|w| w.home.clone()) else {
            return false;
        };
        let Some(ws) = self.workspace_mut(&workspace) else {
            return false;
        };
        let next = big.map(|big| (id.to_owned(), big));
        if ws.big == next || (big.is_none() && ws.big.as_ref().is_none_or(|(w, _)| w != id)) {
            return false;
        }
        ws.big = next;
        if big.is_some() {
            ws.zen = None;
            ws.tape.focus_window(id);
            if ws.floats.iter().any(|w| w == id) {
                raise(&mut ws.floats, id);
                ws.float_focused = true;
            } else {
                ws.float_focused = false;
            }
        }
        true
    }

    /// The window fullscreen on the workspace an output shows.
    pub fn fullscreen_on(&self, output: &str) -> Option<&WinId> {
        let workspace = self.workspace(&self.output(output)?.active)?;
        match &workspace.big {
            Some((id, Big::Fullscreen)) => Some(id),
            _ => None,
        }
    }

    pub fn big_of(&self, id: &str) -> Option<Big> {
        let Home::Workspace(workspace) = &self.window(id)?.home else {
            return None;
        };
        self.workspace(workspace)?
            .big
            .as_ref()
            .filter(|(big, _)| big == id)
            .map(|(_, kind)| *kind)
    }

    pub fn toggle_big(&mut self, id: &str, kind: Big) -> bool {
        let next = (self.big_of(id) != Some(kind)).then_some(kind);
        self.set_big(id, next)
    }

    /// A floating window's rect, global logical.
    pub fn place(&mut self, id: &str, rect: Rect) -> bool {
        let Some(origin) = self
            .output_of(id)
            .and_then(|o| self.output(&o))
            .map(|o| o.geo.rect)
        else {
            return false;
        };
        match self.window_mut(id) {
            Some(window) if window.floating => {
                window.float_rect = rect.translate(-origin.x, -origin.y);
                true
            }
            _ => false,
        }
    }

    /// The output a window is on: its workspace's, or the one showing its
    /// overlay (the focused one while parked).
    pub fn output_of(&self, id: &str) -> Option<String> {
        match &self.window(id)?.home {
            Home::Workspace(workspace) => self.workspace(workspace).map(|ws| ws.output.clone()),
            Home::Overlay(name) => Some(
                self.overlays
                    .iter()
                    .find(|o| &o.name == name)
                    .and_then(|o| o.shown_on.clone())
                    .unwrap_or_else(|| self.focused_output.clone()),
            ),
        }
    }

    /// Sends a window to a workspace. `follow` takes the view with it;
    /// without it the focus stays where it was.
    pub fn move_to_workspace(&mut self, id: &str, workspace: &str, follow: bool) -> bool {
        let Some(window) = self.window(id) else {
            return false;
        };
        if self.workspace(workspace).is_none()
            || window.home == Home::Workspace(workspace.to_owned())
        {
            return false;
        }
        let (from_output, to_output) = (
            self.output_of(id),
            self.workspace(workspace).map(|ws| ws.output.clone()),
        );
        self.detach(id);
        if let Some(window) = self.window_mut(id) {
            if matches!(window.home, Home::Overlay(_)) {
                window.floating = true;
            }
            window.pinned &= from_output == to_output;
        }
        self.attach(id, workspace, None);
        if follow {
            self.show_workspace(workspace);
        }
        self.collect();
        true
    }

    /// Swaps two tiled windows' slots, across workspaces too.
    pub fn swap(&mut self, a: &str, b: &str) -> bool {
        let home = |model: &Model, id: &str| match model.window(id).map(|w| (&w.home, w.floating)) {
            Some((Home::Workspace(ws), false)) => Some(ws.clone()),
            _ => None,
        };
        let (Some(ws_a), Some(ws_b)) = (home(self, a), home(self, b)) else {
            return false;
        };
        if ws_a == ws_b {
            return self
                .workspace_mut(&ws_a)
                .is_some_and(|ws| ws.tape.swap_windows(a, b));
        }
        if let Some(ws) = self.workspace_mut(&ws_a) {
            ws.tape.replace(a, b.to_owned());
        }
        if let Some(ws) = self.workspace_mut(&ws_b) {
            ws.tape.replace(b, a.to_owned());
        }
        for (id, ws) in [(a, ws_b), (b, ws_a)] {
            if let Some(window) = self.window_mut(id) {
                window.home = Home::Workspace(ws);
            }
        }
        true
    }

    /// Moves the column holding `id` before the column holding `before`
    /// (on whichever workspace that is), or to the end of its own tape.
    pub fn move_column(&mut self, id: &str, before: Option<&str>) -> bool {
        let tile_home =
            |model: &Model, id: &str| match model.window(id).map(|w| (&w.home, w.floating)) {
                Some((Home::Workspace(ws), false)) => Some(ws.clone()),
                _ => None,
            };
        let Some(from) = tile_home(self, id) else {
            return false;
        };
        let to = before
            .and_then(|before| tile_home(self, before))
            .unwrap_or_else(|| from.clone());
        if from == to {
            let strip_w = self.strip_w(&from);
            return self.workspace_mut(&from).is_some_and(|ws| {
                let moved = ws.tape.move_column(id, before);
                ws.tape.fit(strip_w);
                moved
            });
        }
        let Some(source) = self.workspace_mut(&from) else {
            return false;
        };
        let Some((index, _)) = source.tape.find(id) else {
            return false;
        };
        let column = source.tape.columns.remove(index);
        source.tape.focus = source
            .tape
            .focus
            .min(source.tape.columns.len().saturating_sub(1));
        let strip_from = self.strip_w(&from);
        if let Some(source) = self.workspace_mut(&from) {
            source.tape.fit(strip_from);
        }
        let moved: Vec<WinId> = column.windows.clone();
        let strip_to = self.strip_w(&to);
        if let Some(target) = self.workspace_mut(&to) {
            let focused = target.tape.focused().cloned();
            let at = before
                .and_then(|before| target.tape.find(before))
                .map_or(target.tape.columns.len(), |(c, _)| c);
            target.tape.columns.insert(at, column);
            match focused {
                Some(focused) => {
                    target.tape.focus_window(&focused);
                }
                None => target.tape.focus = at,
            }
            target.tape.fit(strip_to);
        }
        for window in moved {
            if let Some(window) = self.window_mut(&window) {
                window.home = Home::Workspace(to.clone());
            }
        }
        true
    }

    // Overlays.

    fn overlay_mut(&mut self, name: &str) -> &mut Overlay {
        let index = match self.overlays.iter().position(|o| o.name == name) {
            Some(index) => index,
            None => {
                self.overlays.push(Overlay {
                    name: name.to_owned(),
                    windows: Vec::new(),
                    shown_on: None,
                });
                self.overlays.len() - 1
            }
        };
        &mut self.overlays[index]
    }

    fn hide_overlays_on(&mut self, output: &str) {
        for overlay in &mut self.overlays {
            if overlay.shown_on.as_deref() == Some(output) {
                overlay.shown_on = None;
            }
        }
    }

    /// Moves a window onto an overlay and hides that overlay.
    pub fn park(&mut self, id: &str, overlay: &str) -> bool {
        if self.window(id).is_none() {
            return false;
        }
        self.detach(id);
        if let Some(window) = self.window_mut(id) {
            window.floating = true;
            window.pinned = false;
            window.home = Home::Overlay(overlay.to_owned());
        }
        let overlay = self.overlay_mut(overlay);
        overlay.windows.push(id.to_owned());
        overlay.shown_on = None;
        self.collect();
        true
    }

    /// Brings a window's overlay onto the focused output and focuses the
    /// window.
    pub fn show_overlay(&mut self, id: &str) -> bool {
        let Some(Home::Overlay(name)) = self.window(id).map(|w| w.home.clone()) else {
            return false;
        };
        let output = self.focused_output.clone();
        self.hide_overlays_on(&output);
        let overlay = self.overlay_mut(&name);
        overlay.shown_on = Some(output);
        raise(&mut overlay.windows, id);
        true
    }

    // Workspaces.

    pub fn focus_workspace(&mut self, id: &str) -> bool {
        if self.workspace(id).is_none() {
            return false;
        }
        let changed = self.focused_workspace().is_none_or(|ws| ws.id != id);
        self.show_workspace(id);
        changed
    }

    /// The `idx`-th workspace (from 1) of an output's stack; the focused
    /// output's without one.
    pub fn focus_workspace_at(&mut self, idx: i64, output: Option<&str>) -> bool {
        let output = output.unwrap_or(&self.focused_output).to_owned();
        let Some(id) = usize::try_from(idx - 1)
            .ok()
            .and_then(|index| self.stack(&output).get(index).map(|ws| ws.id.clone()))
        else {
            return false;
        };
        self.focus_workspace(&id)
    }

    /// The neighbour in the focused output's stack; stops at its ends.
    pub fn focus_workspace_relative(&mut self, delta: isize) -> bool {
        let stack = self.stack(&self.focused_output);
        let Some(current) = self
            .focused_workspace()
            .and_then(|ws| stack.iter().position(|w| w.id == ws.id))
        else {
            return false;
        };
        let next = (current as isize + delta).clamp(0, stack.len() as isize - 1) as usize;
        let id = stack[next].id.clone();
        self.focus_workspace(&id)
    }

    /// Hyprland's `e+1`: `delta` steps through the focused output's
    /// workspaces that hold a window, the one in view counted with them,
    /// round from the last to the first.
    pub fn focus_occupied_workspace(&mut self, delta: isize) -> bool {
        let Some(current) = self.focused_workspace().map(|ws| ws.id.clone()) else {
            return false;
        };
        let occupied: Vec<WsId> = self
            .stack(&self.focused_output)
            .into_iter()
            .filter(|ws| !ws.is_empty() || ws.id == current)
            .map(|ws| ws.id.clone())
            .collect();
        let Some(at) = occupied.iter().position(|id| *id == current) else {
            return false;
        };
        let next = (at as isize + delta).rem_euclid(occupied.len() as isize) as usize;
        let id = occupied[next].clone();
        self.focus_workspace(&id)
    }

    pub fn focus_previous_workspace(&mut self) -> bool {
        match self.previous_workspace.clone() {
            Some(id) => self.focus_workspace(&id),
            None => false,
        }
    }

    /// Opens an unnamed workspace at `index` of an output's stack.
    pub fn insert_workspace(&mut self, output: &str, index: usize) -> WsId {
        self.next_dynamic += 1;
        let id = format!("d{}", self.next_dynamic);
        let mut ws = Workspace::new(id.clone(), String::new(), None, false);
        ws.output = output.to_owned();
        let at = self
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, ws)| ws.output == output)
            .map(|(position, _)| position)
            .nth(index)
            .unwrap_or(self.workspaces.len());
        self.workspaces.insert(at, ws);
        id
    }

    /// Gives the focus to an output, as a click on it does.
    pub fn focus_output(&mut self, output: &str) -> bool {
        if self.output(output).is_none() || self.focused_output == output {
            return false;
        }
        self.focused_output = output.to_owned();
        true
    }

    // The focused tape.

    fn tape_op(&mut self, op: impl FnOnce(&mut Tape, &Options) -> bool) -> bool {
        let options = self.options.clone();
        let strip_w = self.strip_of(&self.focused_output).w;
        let Some(ws) = self.focused_workspace_mut() else {
            return false;
        };
        if ws.zen.is_some() {
            return false;
        }
        let was_floating = std::mem::take(&mut ws.float_focused);
        let changed = op(&mut ws.tape, &options);
        if changed {
            let focused = ws.tape.focused().cloned();
            if ws.big.as_ref().map(|(big, _)| big) != focused.as_ref() {
                ws.big = None;
            }
        }
        ws.tape.fit(strip_w);
        changed || (was_floating && !ws.tape.is_empty())
    }

    pub fn focus_column(&mut self, side: Side) -> bool {
        self.tape_op(|tape, _| tape.focus_column(side))
    }

    pub fn focus_row(&mut self, side: Side) -> bool {
        self.tape_op(|tape, _| tape.focus_row(side))
    }

    pub fn swap_column(&mut self, side: Side) -> bool {
        self.tape_op(|tape, _| tape.swap_column(side))
    }

    pub fn swap_row(&mut self, side: Side) -> bool {
        self.tape_op(|tape, _| tape.swap_row(side))
    }

    pub fn consume_or_expel(&mut self, side: Side) -> bool {
        self.tape_op(|tape, _| tape.consume_or_expel(side))
    }

    pub fn set_column_width(&mut self, width: f64) -> bool {
        self.tape_op(|tape, _| tape.set_width(width))
    }

    pub fn resize_column(&mut self, delta: f64) -> bool {
        self.tape_op(|tape, _| tape.resize(delta))
    }

    pub fn cycle_column_width(&mut self) -> bool {
        self.tape_op(|tape, options| tape.cycle_width(&options.presets))
    }

    pub fn fit_into_view(&mut self) -> bool {
        self.tape_op(|_, _| true)
    }

    /// Drags a tape by `dx`; nothing moves while zen holds it.
    pub fn scroll_by(&mut self, workspace: &str, dx: f64) -> bool {
        let strip_w = self.strip_w(workspace);
        let Some(ws) = self.workspace_mut(workspace) else {
            return false;
        };
        if ws.zen.is_some() {
            return false;
        }
        let before = ws.tape.scroll;
        ws.tape.scroll += dx;
        ws.tape.clamp_scroll(strip_w);
        ws.tape.scroll != before
    }

    /// Lets go of a dragged tape heading for `projected`: it snaps to a
    /// column and the focus moves to one still in view.
    pub fn scroll_release(&mut self, workspace: &str, projected: f64) {
        let strip_w = self.strip_w(workspace);
        if let Some(ws) = self.workspace_mut(workspace) {
            if ws.zen.is_some() {
                return;
            }
            ws.tape.scroll = ws.tape.snap(projected, strip_w);
            if ws.tape.focus_in_view(strip_w) {
                ws.float_focused = false;
            }
        }
    }

    // Zen.

    /// `id` absent means the focused window, `on` absent toggles.
    pub fn set_zen(&mut self, id: Option<&str>, on: Option<bool>) -> bool {
        let Some(id) = id
            .map(str::to_owned)
            .or_else(|| self.focused_window().cloned())
        else {
            return false;
        };
        let Some(Home::Workspace(workspace)) = self.window(&id).map(|w| w.home.clone()) else {
            return false;
        };
        let floating = self.window(&id).is_some_and(|w| w.floating);
        let Some(ws) = self.workspace_mut(&workspace) else {
            return false;
        };
        let on = on.unwrap_or(ws.zen.as_deref() != Some(&id));
        if on == (ws.zen.as_deref() == Some(&id)) {
            return false;
        }
        if on {
            ws.zen = Some(id.clone());
            ws.big = None;
            if floating {
                raise(&mut ws.floats, &id);
                ws.float_focused = true;
            } else {
                ws.tape.focus_window(&id);
                ws.float_focused = false;
            }
        } else if ws.zen.as_deref() == Some(&id) {
            ws.zen = None;
        }
        true
    }

    // Dragging a window.

    /// Lifts a tiled window out of its tape; it floats under the pointer
    /// until it is dropped.
    pub fn lift(&mut self, id: &str) -> Option<Lifted> {
        let window = self.window(id)?;
        let Home::Workspace(workspace) = window.home.clone() else {
            return None;
        };
        if window.floating {
            return None;
        }
        let strip_w = self.strip_w(&workspace);
        let ws = self.workspace_mut(&workspace)?;
        if ws.zen.is_some() {
            return None;
        }
        let (column, row) = ws.tape.find(id)?;
        let alone = ws.tape.columns[column].windows.len() == 1;
        let width = ws.tape.remove(id)?;
        ws.tape.fit(strip_w);
        if ws.big.as_ref().is_some_and(|(big, _)| big == id) {
            ws.big = None;
        }
        Some(Lifted {
            id: id.to_owned(),
            workspace,
            column,
            row,
            width,
            alone,
        })
    }

    /// Drops a lifted window as a column at gap `index` of a workspace's
    /// tape and focuses it there.
    pub fn drop_column(&mut self, lifted: &Lifted, workspace: &str, index: usize) {
        let strip_w = self.strip_w(workspace);
        let Some(ws) = self.workspace_mut(workspace) else {
            return self.drop_back(lifted);
        };
        ws.tape.insert_at(index, lifted.id.clone(), lifted.width);
        ws.tape.fit(strip_w);
        ws.float_focused = false;
        ws.big = None;
        if let Some(window) = self.window_mut(&lifted.id) {
            window.home = Home::Workspace(workspace.to_owned());
        }
        self.show_workspace(workspace);
    }

    /// Puts a lifted window back where it came from.
    pub fn drop_back(&mut self, lifted: &Lifted) {
        let strip_w = self.strip_w(&lifted.workspace);
        let Some(ws) = self.workspace_mut(&lifted.workspace) else {
            return;
        };
        if lifted.alone {
            ws.tape
                .insert_at(lifted.column, lifted.id.clone(), lifted.width);
        } else {
            ws.tape
                .insert_into(lifted.column, lifted.row, lifted.id.clone());
        }
        ws.tape.fit(strip_w);
    }

    // Layout.

    /// Every window's place at rest.
    pub fn layout(&self) -> Vec<Placed> {
        let mut placed = Vec::new();
        for ws in &self.workspaces {
            let Some(output) = self.output(&ws.output) else {
                continue;
            };
            let shown = output.active == ws.id;
            let strip = self.strip_of(&ws.output);
            let zen_rect = self.zen_rect(&ws.output);
            let zen_column = ws
                .zen
                .as_ref()
                .and_then(|zen| ws.tape.find(zen))
                .map(|(c, _)| c);
            let view_centre = ws.tape.scroll + strip.w / 2.0;
            let base = |id: &WinId, rect: Rect, layer: Layer, order: usize| Placed {
                id: id.clone(),
                output: ws.output.clone(),
                workspace: Some(ws.id.clone()),
                rect,
                layer,
                order,
                scrolls: false,
                column: -1,
                row: -1,
                shown,
            };
            for (order, cell) in ws
                .tape
                .cells(strip, self.options.gaps_in)
                .into_iter()
                .enumerate()
            {
                let rect = cell.rect.translate(strip.x, strip.y);
                let mut place = base(&cell.id, rect, Layer::Tile, order);
                place.column = cell.column as i32;
                place.row = cell.row as i32;
                place.scrolls = true;
                if ws.zen.as_ref() == Some(&cell.id) {
                    place.rect = zen_rect;
                    place.layer = Layer::Zen;
                    place.scrolls = false;
                } else if ws.zen.is_some() {
                    let left = match zen_column {
                        Some(column) => cell.column < column,
                        None => cell.rect.center().0 < view_centre,
                    };
                    let shift = if left {
                        -output.geo.rect.w
                    } else {
                        output.geo.rect.w
                    };
                    place.rect = rect.translate(shift, 0.0);
                } else if let Some((big, kind)) = &ws.big
                    && big == &cell.id
                {
                    place.scrolls = false;
                    (place.rect, place.layer) = match kind {
                        Big::Maximized => (output.geo.usable, Layer::Maximized),
                        Big::Fullscreen => (output.geo.rect, Layer::Fullscreen),
                    };
                }
                placed.push(place);
            }
            for (order, id) in ws.floats.iter().enumerate() {
                let Some(window) = self.window(id) else {
                    continue;
                };
                let rect = window
                    .float_rect
                    .translate(output.geo.rect.x, output.geo.rect.y);
                let layer = if window.pinned {
                    Layer::Pinned
                } else {
                    Layer::Float
                };
                let mut place = base(id, rect, layer, order);
                if ws.zen.as_ref() == Some(id) {
                    place.rect = zen_rect;
                    place.layer = Layer::Zen;
                } else if let Some((big, kind)) = &ws.big
                    && big == id
                {
                    (place.rect, place.layer) = match kind {
                        Big::Maximized => (output.geo.usable, Layer::Maximized),
                        Big::Fullscreen => (output.geo.rect, Layer::Fullscreen),
                    };
                }
                placed.push(place);
            }
        }
        for overlay in &self.overlays {
            let name = overlay.shown_on.as_ref().unwrap_or(&self.focused_output);
            let Some(output) = self.output(name) else {
                continue;
            };
            for (order, id) in overlay.windows.iter().enumerate() {
                let Some(window) = self.window(id) else {
                    continue;
                };
                placed.push(Placed {
                    id: id.clone(),
                    output: name.clone(),
                    workspace: None,
                    rect: window
                        .float_rect
                        .translate(output.geo.rect.x, output.geo.rect.y),
                    layer: Layer::Overlay,
                    order,
                    scrolls: false,
                    column: -1,
                    row: -1,
                    shown: overlay.shown_on.is_some(),
                });
            }
        }
        placed
    }
}

fn raise(windows: &mut Vec<WinId>, id: &str) {
    if let Some(index) = windows.iter().position(|w| w == id) {
        let window = windows.remove(index);
        windows.push(window);
    }
}
