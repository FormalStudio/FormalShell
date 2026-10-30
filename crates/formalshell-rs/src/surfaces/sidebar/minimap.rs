//! The Spaces minimap: a row per workspace on the sidebar's output, its
//! ordinal and its windows' icons three across in screen order, one empty
//! row under the last for a new workspace. Icons come off compositor
//! metadata through `appicon`; nothing is captured.

use std::time::Instant;

use fs_chrome::bar::workspaces::{self, SlotOpts};
use fs_theme::theme::Theme;
use serde_json::{Value, json};

use crate::scene::Bitmap;
use crate::services::appicon;
use crate::store::Store;
use crate::surfaces::bar::cell::Action;
use crate::surfaces::bar::cells::workspaces::{window, workspace};
use crate::ui::{El, Ink, Variant, w};

/// Icons a row lays across before it wraps.
pub const ACROSS: usize = 3;
/// The cap past which a row stops listing windows.
const MAX: usize = 64;

#[derive(Clone)]
pub struct Win {
    pub id: String,
    pub focused: bool,
    pub icon: Option<Bitmap>,
}

#[derive(Clone)]
pub struct Row {
    /// Empty for a persistent slot the compositor has no workspace for.
    pub id: String,
    pub idx: i64,
    pub label: String,
    pub current: bool,
    pub windows: Vec<Win>,
}

/// The keyboard's place on the map: the window it is on, or a row with no
/// window under it (the empty row is `rows.len()`). It follows its window
/// to wherever the compositor puts it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cursor {
    pub window: Option<String>,
    pub row: usize,
}

/// A carry the compositor is asked to make.
#[derive(Clone, Debug, PartialEq)]
pub enum Move {
    Swap(String, String),
    /// To the workspace with this ordinal.
    To(String, i64),
}

#[derive(Default)]
pub struct Map {
    pub rows: Vec<Row>,
    probed: Option<(u32, Vec<appicon::Query>)>,
    pub cursor: Option<Cursor>,
}

impl Map {
    /// The rows for `output` off the compositor's snapshot.
    pub fn read(&mut self, store: &Store, output: &str, theme: &Theme) {
        let c = &store.compositor.snapshot;
        let ws: Vec<workspaces::Workspace> = c.workspaces.iter().map(workspace).collect();
        let wins: Vec<workspaces::Window> = c.windows.iter().map(window).collect();
        let slots = workspaces::slots(&ws, &wins, output, SlotOpts { persistent: 0, max_icons: MAX });
        let size = icon_size(theme).round() as u32;
        let scale = crate::scene::raster_scale();
        let queries: Vec<appicon::Query> = slots
            .iter()
            .flat_map(|s| &s.windows)
            .map(|w| appicon::Query {
                id: w.id.clone(),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
            })
            .collect();
        let px = crate::scene::pixels_at(f64::from(size), scale);
        if self.probed.as_ref().is_none_or(|(s, q)| *s != px || *q != queries) {
            self.probed = Some((px, queries.clone()));
            appicon::probe(size, scale, queries);
        }
        self.rows = slots
            .into_iter()
            .map(|s| Row {
                windows: s
                    .windows
                    .iter()
                    .map(|w| Win { id: w.id.clone(), focused: w.is_focused, icon: store.appicon.get(&w.id, size, scale).and_then(|i| i.image) })
                    .collect(),
                id: s.id,
                idx: s.idx,
                label: s.label,
                current: s.current,
            })
            .collect();
        if let Some(c) = &mut self.cursor {
            match c.window.as_ref().and_then(|id| self.rows.iter().position(|r| r.windows.iter().any(|w| &w.id == id))) {
                Some(row) => c.row = row,
                None => {
                    c.window = None;
                    c.row = c.row.min(self.rows.len());
                }
            }
        }
    }

    /// The cursor's row and its column in that row.
    fn at(&self) -> Option<(usize, usize)> {
        let c = self.cursor.as_ref()?;
        let col = c.window.as_ref().and_then(|id| self.rows.get(c.row)?.windows.iter().position(|w| &w.id == id)).unwrap_or(0);
        Some((c.row, col))
    }

    /// The cursor on the focused window, else on the current row.
    pub fn take_keyboard(&mut self) {
        let row = self.rows.iter().position(|r| r.current).unwrap_or(0);
        let window = self.rows.get(row).and_then(|r| r.windows.iter().find(|w| w.focused).or(r.windows.first())).map(|w| w.id.clone());
        self.cursor = Some(Cursor { window, row });
    }

    /// An arrow: the cursor moves, or with `carry` the window under it does
    /// (along its row a swap with its neighbour, across rows onto that
    /// workspace, past the last onto a new one).
    pub fn step(&mut self, dx: i32, dy: i32, carry: bool) -> Option<Move> {
        let (row, col) = self.at()?;
        let windows = |r: usize| self.rows.get(r).map_or(&[][..], |r| r.windows.as_slice());
        if carry {
            let id = self.cursor.as_ref()?.window.clone()?;
            if dx != 0 {
                let other = windows(row).get(col.checked_add_signed(dx as isize)?)?.id.clone();
                return Some(Move::Swap(id, other));
            }
            let to = row.checked_add_signed(dy as isize)?;
            let idx = match self.rows.get(to) {
                Some(r) => r.idx,
                None if to == self.rows.len() && windows(row).len() > 1 => self.next_idx(),
                None => return None,
            };
            if let Some(c) = &mut self.cursor {
                c.row = to;
            }
            return Some(Move::To(id, idx));
        }
        let (row, col) = if dx != 0 {
            (row, col.saturating_add_signed(dx as isize).min(windows(row).len().saturating_sub(1)))
        } else {
            (row.saturating_add_signed(dy as isize).min(self.rows.len()), col)
        };
        let window = windows(row).get(col.min(windows(row).len().saturating_sub(1))).map(|w| w.id.clone());
        self.cursor = Some(Cursor { window, row });
        None
    }

    /// Enter on the cursor: its window, its row's workspace, or a new one.
    pub fn enter(&self, output: &str) -> Action {
        let Some(c) = &self.cursor else { return Action::None };
        match (&c.window, self.rows.get(c.row)) {
            (Some(id), _) => Action::Window(id.clone()),
            (None, Some(_)) => self.action(&format!("row:{}", c.row), output),
            (None, None) => self.action("new", output),
        }
    }

    /// The cursor's stop, for the ring.
    pub fn cursor_key(&self) -> Option<String> {
        let c = self.cursor.as_ref()?;
        Some(match &c.window {
            Some(id) => format!("w:{id}"),
            None if c.row < self.rows.len() => format!("r:{}", c.row),
            None => "new".into(),
        })
    }

    /// The ordinal a window dropped on the empty row gets.
    pub fn next_idx(&self) -> i64 {
        self.rows.iter().map(|r| r.idx).max().unwrap_or(0) + 1
    }

    pub fn view(&self, theme: &Theme, _now: Instant) -> El {
        let s = &theme.space;
        let size = icon_size(theme);
        let mut col: Vec<El> = self.rows.iter().enumerate().map(|(i, r)| row(theme, i, r, size)).collect();
        let empty = w::row(s.sm, vec![w::caption(format!("{}", self.next_idx())).mono().ink(Ink::Dim), w::spacer()]);
        col.push(w::cell(empty).ghost().interactive().on("new").stop("new").fill());
        w::column(s.xxs, col).fill()
    }

    /// The rail's chips: one ordinal per workspace, the focused one filled.
    pub fn rail(&self, theme: &Theme) -> El {
        let chips = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let v = if r.current { Variant::Default } else { Variant::Ghost };
                w::button(r.label.clone()).variant(v).on(format!("row:{i}")).centred()
            })
            .collect();
        w::column(theme.space.xxs, chips).fill()
    }

    pub fn action(&self, on: &str, output: &str) -> Action {
        let (verb, arg) = on.split_once(':').unwrap_or((on, ""));
        match verb {
            "window" => Action::Window(arg.to_owned()),
            "row" => match arg.parse::<usize>().ok().and_then(|i| self.rows.get(i)) {
                Some(r) if r.id.is_empty() => Action::WorkspaceAt(r.idx, output.to_owned()),
                Some(r) => Action::Workspace(r.id.clone()),
                None => Action::None,
            },
            "new" => Action::WorkspaceAt(self.next_idx(), output.to_owned()),
            _ => Action::None,
        }
    }

    pub fn status(&self) -> Value {
        Value::Array(
            self.rows
                .iter()
                .map(|r| json!({"id": r.id, "idx": r.idx, "current": r.current, "windows": r.windows.iter().map(|w| &w.id).collect::<Vec<_>>()}))
                .collect(),
        )
    }

    pub fn cursor_status(&self) -> Value {
        self.cursor.as_ref().map_or(Value::Null, |c| json!({"window": c.window, "row": c.row}))
    }
}

/// An icon three across a sidebar card, past the ordinal.
fn icon_size(theme: &Theme) -> f64 {
    theme.space.control_height * 0.75
}

fn row(theme: &Theme, i: usize, r: &Row, size: f64) -> El {
    let s = &theme.space;
    let icons: Vec<El> = r
        .windows
        .iter()
        .map(|win| {
            let pic = match &win.icon {
                Some(b) => w::picture(Some(b.clone()), size),
                None => w::icon("app-window"),
            };
            w::cell(pic).cell_state(|c| {
                c.chip = true;
                c.small = true;
                c.ghost = !win.focused;
                c.selected = win.focused;
            })
            .interactive()
            .on(format!("window:{}", win.id))
            .stop(format!("w:{}", win.id))
        })
        .collect();
    let label = w::caption(r.label.clone()).mono().ink(if r.current { Ink::Fg } else { Ink::Dim });
    let body = w::row(s.sm, vec![label, w::grid(ACROSS, s.xxs, icons).fill()]);
    w::cell(body).ghost().selected(r.current).interactive().on(format!("row:{i}")).stop(format!("r:{i}")).fill()
}
