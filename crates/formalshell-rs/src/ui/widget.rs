//! Custom widgets: a feature's own drawing (a panel's diagram, its
//! spectrum) as one element of the tree, living beside the panel that
//! uses it. The kit lays it out by its `measure` and hands it its box to
//! `paint`; whatever it keeps between draws lives in `Ui` by its path
//! ([`Cx::state`]) and goes once the element has not been drawn for a while.

use std::any::Any;
use std::fmt::Debug;

use vello_cpu::kurbo::Rect;

use super::Cx;

pub trait Widget: Any + Debug + Same {
    /// Its natural (width, height) at `avail` wide, padding not included.
    fn measure(&self, cx: &mut Cx, avail: f64) -> (f64, f64);
    /// Paints into `inner`, the element's box inside its padding.
    fn paint(&self, cx: &mut Cx, inner: Rect, path: &str);
}

/// What a widget keeps between draws.
pub trait State: Any {
    /// A wheel's `delta` px over the widget's scroll region.
    fn scroll(&mut self, _delta: f64) {}
}

/// Clone and compare through the box, so a tree holding a widget is plain
/// data like any other.
pub trait Same {
    fn boxed(&self) -> Box<dyn Widget>;
    fn same(&self, other: &dyn Widget) -> bool;
}

impl<T: Widget + Clone + PartialEq + 'static> Same for T {
    fn boxed(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }

    fn same(&self, other: &dyn Widget) -> bool {
        (other as &dyn Any).downcast_ref::<T>() == Some(self)
    }
}

impl Clone for Box<dyn Widget> {
    fn clone(&self) -> Self {
        self.boxed()
    }
}

impl PartialEq for Box<dyn Widget> {
    fn eq(&self, other: &Self) -> bool {
        self.same(&**other)
    }
}
