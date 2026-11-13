mod common;

use std::collections::BTreeMap;

use common::*;
use formalwm_core::{
    geom::Rect,
    model::{Layer, Placement},
    motion::{Clocks, Spring, SpringParams},
    scene::Scene,
};
use fs_wm_proto::Motion;

const FRAME: f64 = 1.0 / 120.0;

fn settle(scene: &mut Scene) -> usize {
    let mut frames = 0;
    while scene.animating() {
        scene.step(FRAME);
        frames += 1;
        assert!(frames < 2400, "the scene never came to rest");
    }
    frames
}

/// Opens a tile its client has not drawn yet, as the engine does: the model
/// takes it, but its tape keeps the scroll it had until it draws.
fn open_waiting(model: &mut formalwm_core::model::Model, scene: &mut Scene, id: &str) {
    let before = model.workspace("1").unwrap().tape.scroll;
    open(model, id);
    let now = model.workspace("1").unwrap().tape.scroll;
    model.scroll_by("1", before - now);
    scene.wait(id);
}

/// The waited tile has drawn: the view fits to it if it has the focus.
fn arrive(model: &mut formalwm_core::model::Model, scene: &mut Scene, id: &str) {
    scene.arrive(id);
    if model.focused_window().is_some_and(|focus| focus == id) {
        model.focus_window(id);
    }
}

fn drawn(scene: &Scene, id: &str) -> Rect {
    scene.visual(id).unwrap().rect
}

#[test]
fn at_rest_every_window_is_where_the_model_has_it() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
        scene.sync(&model);
    }
    settle(&mut scene);
    let scroll = model.workspace("1").unwrap().tape.scroll;
    for id in ["a", "b", "c"] {
        let visual = scene.visual(id).unwrap();
        let rest = place(&model, id).rect.translate(-scroll, 0.0);
        assert_eq!(visual.rect, rest, "{id}");
        assert_eq!(visual.size, (rest.w, rest.h));
        assert_eq!(visual.opacity, 1.0);
        assert!(visual.settled);
    }
    assert!(
        !scene.visual("a").unwrap().visible,
        "a is scrolled out of view"
    );
    assert!(scene.visual("c").unwrap().visible);
}

#[test]
fn a_new_window_pops_in_and_fades_up() {
    let mut model = desk();
    let mut scene = Scene::default();
    open(&mut model, "a");
    scene.sync(&model);
    let first = scene.visual("a").unwrap();
    assert_eq!(first.opacity, 0.0);
    assert_eq!((first.rect.w, first.rect.h), (5.0, 5.0));
    assert_eq!(first.rect.center(), place(&model, "a").rect.center());
    scene.step(0.05);
    let mid = scene.visual("a").unwrap();
    assert!(mid.opacity > 0.0 && mid.opacity < 1.0);
    assert!(mid.rect.w > first.rect.w);
    // The client's first picture starts it over.
    scene.arrive("a");
    assert_eq!(scene.visual("a").unwrap(), first);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "a"), place(&model, "a").rect);
}

#[test]
fn a_window_that_changes_place_starts_from_where_it_is_drawn() {
    let mut model = desk();
    let mut scene = Scene::default();
    open(&mut model, "a");
    open(&mut model, "b");
    scene.sync(&model);
    settle(&mut scene);
    let before = drawn(&scene, "a");
    model.focus_window("a");
    model.set_column_width(1.0);
    scene.sync(&model);
    assert_eq!(
        drawn(&scene, "a"),
        before,
        "nothing jumps on the frame it changes"
    );
    scene.step(0.1);
    let mid = drawn(&scene, "a");
    assert!(mid.w > before.w && mid.w < 1904.0);
    assert_eq!(
        scene.visual("a").unwrap().size,
        (1904.0, 1064.0),
        "the client has its final size at once"
    );
    settle(&mut scene);
    assert_eq!(drawn(&scene, "a"), Rect::new(8.0, 8.0, 1904.0, 1064.0));
}

#[test]
fn a_workspace_switch_slides_both_vertically() {
    let mut model = desk();
    let mut scene = Scene::default();
    open(&mut model, "a");
    model.focus_workspace("2");
    open(&mut model, "b");
    scene.sync(&model);
    settle(&mut scene);
    assert!(!scene.visual("a").unwrap().visible);
    assert_eq!(drawn(&scene, "a").y, 8.0 - 1080.0);

    model.focus_workspace("1");
    scene.sync(&model);
    scene.step(0.08);
    let (a, b) = (scene.visual("a").unwrap(), scene.visual("b").unwrap());
    assert!(a.visible && b.visible, "both are on screen mid-slide");
    assert!(a.rect.y < 8.0 && a.rect.y > 8.0 - 1080.0);
    assert!(
        (b.rect.y - a.rect.y - 1080.0).abs() < 1e-6,
        "they move as one stack"
    );
    assert_eq!(a.rect.x, b.rect.x);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "a").y, 8.0);
    assert!(!scene.visual("b").unwrap().visible);
}

#[test]
fn a_held_tape_is_under_the_fingers_and_a_release_carries() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b", "c", "d"] {
        open(&mut model, id);
    }
    model.focus_window("a");
    scene.sync(&model);
    settle(&mut scene);
    for _ in 0..10 {
        model.scroll_by("1", 30.0);
        scene.sync(&model);
        scene.hold_scroll("1");
    }
    assert_eq!(scene.scroll_of("1"), 300.0);
    assert_eq!(drawn(&scene, "a").x, 8.0 - 300.0);
    assert!(!scene.animating(), "held, not animated");

    model.scroll_release("1", 300.0 + 800.0);
    scene.sync(&model);
    scene.release_scroll("1", 2400.0);
    scene.step(FRAME);
    assert!(
        scene.scroll_of("1") > 300.0 + 2400.0 * FRAME * 0.8,
        "it left at the fingers' speed"
    );
    settle(&mut scene);
    assert_eq!(scene.scroll_of("1"), 956.0);
}

#[test]
fn a_held_stack_follows_the_fingers_past_the_neighbour() {
    let mut model = desk();
    let mut scene = Scene::default();
    open(&mut model, "a");
    scene.sync(&model);
    settle(&mut scene);
    scene.hold_stack("A", 1.6);
    assert_eq!(drawn(&scene, "a").y, 8.0 - 1.6 * 1080.0);
    model.focus_workspace("3");
    scene.sync(&model);
    scene.release_stack("A", 2.0);
    settle(&mut scene);
    assert_eq!(scene.stack_of("A"), 2.0);
}

#[test]
fn the_overlay_drops_in_from_the_top() {
    let mut model = desk();
    let mut scene = Scene::default();
    let rule = Placement {
        overlay: Some("formalshell-console".into()),
        float_rect: Some(Rect::new(240.0, 0.0, 1440.0, 400.0)),
        ..Placement::default()
    };
    open_with(&mut model, "console", rule);
    scene.sync(&model);
    settle(&mut scene);
    assert!(!scene.visual("console").unwrap().visible);
    model.show_overlay("console");
    scene.sync(&model);
    scene.step(0.15);
    let mid = scene.visual("console").unwrap();
    assert!(
        mid.visible && mid.rect.y != 0.0,
        "on its way down at {}",
        mid.rect.y
    );
    settle(&mut scene);
    assert_eq!(
        drawn(&scene, "console"),
        Rect::new(240.0, 0.0, 1440.0, 400.0)
    );
}

#[test]
fn a_pinned_window_does_not_ride_the_stack() {
    let mut model = desk();
    let mut scene = Scene::default();
    let rule = Placement {
        floating: true,
        pinned: true,
        float_rect: Some(Rect::new(1300.0, 700.0, 600.0, 360.0)),
        ..Placement::default()
    };
    open_with(&mut model, "pip", rule);
    scene.sync(&model);
    settle(&mut scene);
    model.focus_workspace("5");
    scene.sync(&model);
    for _ in 0..30 {
        scene.step(FRAME);
        assert_eq!(drawn(&scene, "pip"), Rect::new(1300.0, 700.0, 600.0, 360.0));
    }
}

#[test]
fn with_motion_off_everything_lands_at_once() {
    let mut model = desk();
    let mut scene = Scene::new(&Motion {
        enabled: false,
        ..Motion::default()
    });
    open(&mut model, "a");
    scene.sync(&model);
    assert!(!scene.animating());
    assert_eq!(drawn(&scene, "a"), place(&model, "a").rect);
    assert_eq!(scene.visual("a").unwrap().opacity, 1.0);
    model.focus_workspace("4");
    scene.sync(&model);
    assert_eq!(scene.stack_of("A"), 3.0);
    assert_eq!(scene.close_ms(), 0.0);
}

#[test]
fn zen_slides_the_neighbours_off_and_back() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b"] {
        open(&mut model, id);
    }
    scene.sync(&model);
    settle(&mut scene);
    let rest = drawn(&scene, "a");
    model.set_zen(Some("b"), Some(true));
    scene.sync(&model);
    assert_eq!(drawn(&scene, "a"), rest);
    settle(&mut scene);
    assert!(!scene.visual("a").unwrap().visible);
    assert_eq!(drawn(&scene, "b"), model.zen_rect("A"));
    model.set_zen(Some("b"), Some(false));
    scene.sync(&model);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "a"), rest);
}

#[test]
fn zen_hands_over_by_sliding_each_window_from_where_it_is_drawn() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    model.focus_window("b");
    model.set_zen(None, None);
    scene.sync(&model);
    settle(&mut scene);
    let zen = model.zen_rect("A");
    let waiting = drawn(&scene, "c");
    assert!(!scene.visual("c").unwrap().visible);

    model.focus_column(formalwm_core::tape::Side::Next);
    scene.sync(&model);
    assert_eq!(drawn(&scene, "b"), zen, "b leaves from the centre");
    assert_eq!(drawn(&scene, "c"), waiting, "c comes in from off the right");
    for _ in 0..4 {
        scene.step(FRAME);
    }
    let (b, c) = (drawn(&scene, "b"), drawn(&scene, "c"));
    assert!(b.x < zen.x, "b heads left: {b:?}");
    assert!(c.x < waiting.x, "c heads in: {c:?}");
    settle(&mut scene);
    assert_eq!(drawn(&scene, "c"), zen);
    assert!(!scene.visual("b").unwrap().visible);
    assert!(scene.visual("c").unwrap().visible);
}

/// A tile its client has not drawn yet holds its workspace by
/// `Placed::tiled`, laid out as if it were not there. One taking zen as it
/// opens is drawn centred and does not scroll, yet it is a tile like any
/// other: without it, the zen window before it stays in front.
#[test]
fn a_new_tile_taking_zen_holds_its_workspace_with_the_zen_window_before_in_front() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b"] {
        open(&mut model, id);
    }
    model.set_zen(None, None);
    scene.sync(&model);
    settle(&mut scene);
    let zen = model.zen_rect("A");
    assert_eq!(drawn(&scene, "b"), zen);

    open(&mut model, "c");
    let layout = model.layout();
    let c = layout.iter().find(|p| p.id == "c").unwrap();
    assert_eq!(c.layer, Layer::Zen, "c takes zen as it opens");
    assert!(c.tiled() && !c.scrolls, "and counts as a tile though it does not scroll");

    let mut without = model.clone();
    without.close("c");
    let b = without.layout().into_iter().find(|p| p.id == "b").unwrap();
    assert_eq!((b.layer, b.rect), (Layer::Zen, zen), "without c, b keeps zen");
    scene.sync(&without);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "b"), zen, "b stays in front while c waits");
}

#[test]
fn a_dropped_window_rides_to_its_place_from_where_the_hand_left_it() {
    let mut model = common::desk();
    common::open(&mut model, "a");
    let mut scene = Scene::default();
    scene.sync(&model);
    scene.finish();
    let rest = scene.visual("a").unwrap().rect;
    let hand = Rect::new(rest.x + 300.0, rest.y + 120.0, rest.w, rest.h);
    scene.carry("a", hand);
    assert_eq!(scene.visual("a").unwrap().rect, hand);
    for _ in 0..240 {
        scene.step(1.0 / 60.0);
    }
    assert_eq!(scene.visual("a").unwrap().rect, rest);
    assert!(!scene.animating());
}

/// Two outputs refreshing at 144 and 120 Hz, out of phase, each frame
/// prepared two of its own periods ahead as the compositor's render-ahead
/// does: in the order the frames are prepared, `(output, frame time ms)`.
fn interleaved_frames(count: usize) -> Vec<(&'static str, f64)> {
    let mut frames: Vec<(f64, &'static str, f64)> = Vec::new();
    for (output, hz, phase) in [("A", 144.0, 1.3), ("B", 120.0, 5.9)] {
        let period = 1000.0 / hz;
        for n in 0..count {
            let at = 1000.0 + phase + n as f64 * period;
            frames.push((at - 2.0 * period, output, at));
        }
    }
    frames.sort_by(|left, right| left.0.total_cmp(&right.0));
    frames
        .into_iter()
        .map(|(_, output, at)| (output, at))
        .collect()
}

#[test]
fn each_output_steps_on_its_own_frames() {
    let mut model = two_outputs();
    let mut scene = Scene::default();
    open(&mut model, "a");
    model.focus_workspace("2");
    open(&mut model, "b");
    model.focus_workspace("8");
    open(&mut model, "c");
    model.focus_workspace("4");
    scene.sync(&model);
    settle(&mut scene);

    model.focus_workspace("1");
    model.focus_workspace("8");
    scene.sync(&model);
    let params = SpringParams::from_family(&Motion::default().families.spatial);
    let mut ideal: BTreeMap<&str, (Spring, Option<f64>)> = ["A", "B"]
        .into_iter()
        .map(|output| {
            let target = model
                .stack_index(&model.output(output).unwrap().active)
                .unwrap() as f64;
            let mut spring = Spring::at(scene.stack_of(output));
            spring.target = target;
            (output, (spring, None))
        })
        .collect();
    assert!(ideal.values().all(|(spring, _)| !spring.resting()));

    let mut clocks = Clocks::default();
    let mut frames = 0;
    for (output, at) in interleaved_frames(400) {
        let Some(dt) = clocks.advance(output, at) else {
            continue;
        };
        scene.step_on(output, dt);
        // The same spring alone, stepped from this output's last frame time
        // to this one: the curve sampled where this output shows it.
        let (spring, last) = ideal.get_mut(output).unwrap();
        if let Some(last) = *last {
            spring.step_in(params, (at - last) / 1000.0, 0.001);
        }
        *last = Some(at);
        assert!(
            (scene.stack_of(output) - spring.value).abs() < 1e-9,
            "{output} at {at:.2} ms shows {} where its own clock has {}",
            scene.stack_of(output),
            spring.value
        );
        frames += 1;
    }
    assert!(frames > 100);
    assert!(!scene.animating(), "both landed");
}

fn picture(rect: Rect) -> Placement {
    Placement {
        floating: true,
        pip: true,
        float_rect: Some(rect),
        ..Placement::default()
    }
}

#[test]
fn a_picture_does_not_ride_the_stack_and_flies_on_its_own_spring() {
    let mut model = desk();
    let mut scene = Scene::default();
    let from = Rect::new(1424.0, 794.0, 480.0, 270.0);
    open_with(&mut model, "pip", picture(from));
    open(&mut model, "a");
    scene.sync(&model);
    settle(&mut scene);
    model.focus_workspace("5");
    scene.sync(&model);
    for _ in 0..30 {
        scene.step(FRAME);
        assert_eq!(drawn(&scene, "pip"), from);
    }
    assert_eq!(scene.visual("pip").unwrap().lane, 0.0);

    // To the other corner: it and a float making the same move part ways,
    // the picture on its spring and the float on the theme's.
    let float = Placement {
        floating: true,
        float_rect: Some(from),
        ..Placement::default()
    };
    open_with(&mut model, "f", float);
    scene.sync(&model);
    settle(&mut scene);
    let to = Rect::new(16.0, 794.0, 480.0, 270.0);
    model.place("pip", to);
    model.place("f", to);
    scene.sync(&model);
    let mut peak = f64::MAX;
    for _ in 0..12 {
        scene.step(FRAME);
    }
    assert_ne!(drawn(&scene, "pip").x, drawn(&scene, "f").x);
    for _ in 0..240 {
        scene.step(FRAME);
        peak = peak.min(drawn(&scene, "pip").x);
    }
    assert_eq!(drawn(&scene, "pip"), to);
    assert!(peak < 16.0 && peak > 16.0 - 1408.0 * 0.02, "overshoot {peak}");
}

#[test]
fn a_held_picture_is_drawn_where_the_hand_has_it_until_it_lets_go() {
    let mut model = desk();
    let mut scene = Scene::default();
    let rest = Rect::new(1424.0, 794.0, 480.0, 270.0);
    open_with(&mut model, "pip", picture(rest));
    scene.sync(&model);
    settle(&mut scene);

    // Stretched past its size by a pinch: the client keeps `rest`.
    let stretched = Rect::new(1400.0, 780.0, 528.0, 297.0);
    scene.hold("pip", stretched);
    assert!(!scene.animating(), "a held picture waits for the hand");
    for _ in 0..30 {
        scene.step(FRAME);
    }
    assert_eq!(drawn(&scene, "pip"), stretched);
    assert_eq!(scene.visual("pip").unwrap().size, (480.0, 270.0));
    // Something else changing the model does not let it go.
    open(&mut model, "a");
    scene.sync(&model);
    scene.finish();
    assert_eq!(drawn(&scene, "pip"), stretched);

    scene.release("pip");
    assert!(scene.animating());
    scene.step(FRAME);
    let on_the_way = drawn(&scene, "pip");
    assert!(on_the_way.w < 528.0 && on_the_way.w > 480.0);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "pip"), rest);
}

#[test]
fn a_flung_picture_leaves_at_the_speed_of_the_hand() {
    let mut model = desk();
    let mut scene = Scene::default();
    let hand = Rect::new(900.0, 400.0, 480.0, 270.0);
    open_with(&mut model, "pip", picture(hand));
    scene.sync(&model);
    settle(&mut scene);
    let to = Rect::new(16.0, 794.0, 480.0, 270.0);
    model.place("pip", to);
    scene.sync(&model);
    let mut slow = scene.clone();
    scene.fling("pip", (-3000.0, 1500.0));
    scene.step(FRAME);
    slow.step(FRAME);
    let (flung, eased) = (drawn(&scene, "pip"), drawn(&slow, "pip"));
    // One 120 Hz frame at 3000 px/s is 25 px more than the spring covers
    // from rest.
    assert!((eased.x - flung.x - 25.0).abs() < 3.0, "{}", eased.x - flung.x);
    assert!(flung.y - hand.y > eased.y - hand.y);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "pip"), to);

    // Motion off: no flight at all.
    let mut still = Scene::new(&Motion {
        enabled: false,
        ..Motion::default()
    });
    still.sync(&model);
    model.place("pip", hand);
    still.sync(&model);
    still.fling("pip", (-3000.0, 0.0));
    assert!(!still.animating());
    assert_eq!(drawn(&still, "pip"), hand);
}

#[test]
fn a_new_tile_moves_its_workspace_only_once_it_has_drawn() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b"] {
        open(&mut model, id);
        scene.sync(&model);
    }
    model.focus_window("a");
    scene.sync(&model);
    settle(&mut scene);
    let (a, b) = (drawn(&scene, "a"), drawn(&scene, "b"));
    let scroll = scene.scroll_of("1");

    // Opened right of a, as a client's first commit has it placed.
    open_waiting(&mut model, &mut scene, "c");
    scene.sync(&model);
    settle(&mut scene);
    assert_eq!(drawn(&scene, "a"), a);
    assert_eq!(drawn(&scene, "b"), b, "b stays while c has not drawn");
    assert_eq!(scene.scroll_of("1"), scroll);

    arrive(&mut model, &mut scene, "c");
    scene.sync(&model);
    scene.step(FRAME);
    assert!(drawn(&scene, "b").x > b.x, "b makes room once c has drawn");
    let rest = |id: &str| {
        let scroll = model.workspace("1").unwrap().tape.scroll;
        place(&model, id).rect.translate(-scroll, 0.0)
    };
    let (c, at) = (drawn(&scene, "c"), rest("c"));
    assert!(
        (c.x + c.w / 2.0 - (at.x + at.w / 2.0)).abs() < 1.0,
        "c pops in at its place: {c:?}, {at:?}"
    );
    settle(&mut scene);
    assert_eq!(drawn(&scene, "b"), rest("b"));
    assert_eq!(drawn(&scene, "c"), rest("c"));
}

#[test]
fn a_tile_let_go_before_it_draws_frees_its_workspace() {
    let mut model = desk();
    let mut scene = Scene::default();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_window("a");
    scene.sync(&model);
    settle(&mut scene);
    open_waiting(&mut model, &mut scene, "c");
    scene.sync(&model);
    settle(&mut scene);
    let b = drawn(&scene, "b");
    scene.unwait("c");
    scene.sync(&model);
    settle(&mut scene);
    assert_ne!(drawn(&scene, "b"), b);

    // Closed before it drew: the hold goes with it.
    open_waiting(&mut model, &mut scene, "d");
    scene.sync(&model);
    model.close("d");
    scene.sync(&model);
    settle(&mut scene);
    let scroll = model.workspace("1").unwrap().tape.scroll;
    assert_eq!(
        drawn(&scene, "b"),
        place(&model, "b").rect.translate(-scroll, 0.0)
    );
}

#[test]
fn motion_already_running_goes_on_while_a_new_tile_waits() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
        scene.sync(&model);
    }
    settle(&mut scene);
    // Focus back on a scrolls the tape and a resize slides b and c.
    model.focus_window("a");
    model.resize_column(0.1);
    scene.sync(&model);
    for _ in 0..5 {
        scene.step(FRAME);
    }
    let heading = model.workspace("1").unwrap().tape.scroll;
    let rest = |model: &formalwm_core::model::Model, id: &str| {
        let scroll = model.workspace("1").unwrap().tape.scroll;
        place(model, id).rect.translate(-scroll, 0.0)
    };
    let b = rest(&model, "b");
    let (scroll, b_now) = (scene.scroll_of("1"), drawn(&scene, "b"));
    assert_ne!(scroll, heading, "the tape is on its way");
    assert_ne!(b_now, b, "b is on its way");

    open_waiting(&mut model, &mut scene, "d");
    scene.sync(&model);
    scene.step(FRAME);
    assert_ne!(scene.scroll_of("1"), scroll, "the scroll goes on");
    assert_ne!(drawn(&scene, "b"), b_now, "b goes on");
    settle(&mut scene);
    assert_eq!(scene.scroll_of("1"), heading);
    assert_eq!(
        drawn(&scene, "b"),
        b,
        "b ends where it was going, not past d"
    );
}

#[test]
fn a_maximize_on_a_waiting_tiles_workspace_goes_through() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b"] {
        open(&mut model, id);
        scene.sync(&model);
    }
    model.focus_window("a");
    scene.sync(&model);
    settle(&mut scene);
    let b = drawn(&scene, "b");

    open_waiting(&mut model, &mut scene, "c");
    scene.sync(&model);
    model.set_big("a", Some(formalwm_core::model::Big::Maximized));
    scene.sync(&model);
    settle(&mut scene);
    let scroll = model.workspace("1").unwrap().tape.scroll;
    assert_eq!(
        drawn(&scene, "a"),
        place(&model, "a").rect.translate(-scroll, 0.0),
        "a is maximized while c waits"
    );
    assert_eq!(scene.visual("a").unwrap().size, {
        let rect = place(&model, "a").rect;
        (rect.w, rect.h)
    });
    let mut without = model.clone();
    without.close("c");
    assert_eq!(
        drawn(&scene, "b"),
        place(&without, "b").rect.translate(-scroll, 0.0),
        "b makes way for a as it would were c not there"
    );
    assert_ne!(drawn(&scene, "b"), b);
}

/// a, b and c drawn at the default width, c focused and the tape scrolled.
fn scrolled_three() -> (formalwm_core::model::Model, Scene) {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
        scene.sync(&model);
    }
    settle(&mut scene);
    assert!(model.workspace("1").unwrap().tape.scroll > 0.0, "scrolled");
    (model, scene)
}

#[test]
fn a_second_tile_waiting_does_not_scroll_the_first_ones_view() {
    let (mut model, mut scene) = scrolled_three();
    let scroll = scene.scroll_of("1");
    let before: Vec<Rect> = ["a", "b", "c"].map(|id| drawn(&scene, id)).to_vec();

    for id in ["d", "e"] {
        open_waiting(&mut model, &mut scene, id);
        scene.sync(&model);
    }
    settle(&mut scene);
    assert_eq!(scene.scroll_of("1"), scroll, "neither d nor e has drawn");
    let after: Vec<Rect> = ["a", "b", "c"].map(|id| drawn(&scene, id)).to_vec();
    assert_eq!(after, before);
}

#[test]
fn a_maximize_from_a_scrolled_tape_shows_the_window_while_a_tile_waits() {
    let (mut model, mut scene) = scrolled_three();
    open_waiting(&mut model, &mut scene, "d");
    scene.sync(&model);
    settle(&mut scene);

    model.focus_window("a");
    model.set_big("a", Some(formalwm_core::model::Big::Maximized));
    scene.sync(&model);
    settle(&mut scene);
    let a = place(&model, "a");
    assert_eq!(
        drawn(&scene, "a"),
        model.shown_rect(&a),
        "a covers the view as the model has it"
    );
    assert_eq!(
        scene.scroll_of("1"),
        model.workspace("1").unwrap().tape.scroll
    );
}

#[test]
fn a_swipe_while_a_tile_waits_moves_the_view_with_the_fingers_and_lands_with_the_focus() {
    let mut model = desk();
    let mut scene = Scene::default();
    for id in ["a", "b", "c", "e", "f"] {
        open(&mut model, id);
        scene.sync(&model);
    }
    model.focus_window("c");
    scene.sync(&model);
    open_waiting(&mut model, &mut scene, "d");
    scene.sync(&model);
    settle(&mut scene);
    let view = scene.scroll_of("1");
    assert!(view > 400.0, "room to swipe back: {view}");

    // Each step of the fingers moves the view by as much, d not drawn.
    let mut last = view;
    for _ in 0..4 {
        assert!(model.scroll_by("1", -100.0));
        scene.sync(&model);
        settle(&mut scene);
        let now = scene.scroll_of("1");
        assert!((last - now - 100.0).abs() < 1e-6, "{last} to {now}");
        last = now;
    }

    // Let go: the model snaps and focuses a tile in view, which the view
    // shows where the model has it.
    let projected = model.workspace("1").unwrap().tape.scroll - 150.0;
    model.scroll_release("1", projected);
    scene.sync(&model);
    settle(&mut scene);
    let focus = model.focused_window().unwrap().clone();
    assert_ne!(focus, "d");
    let at = model.shown_rect(&place(&model, &focus));
    let drawn_at = drawn(&scene, &focus);
    assert_eq!(
        drawn_at.center(),
        at.center(),
        "{focus} where the model has it"
    );
    assert!(
        drawn_at.x >= 0.0 && drawn_at.x + drawn_at.w <= 1920.0,
        "in view"
    );
}

#[test]
fn the_newest_waiting_tile_drawing_first_is_shown_while_an_older_one_waits() {
    let (mut model, mut scene) = scrolled_three();
    for id in ["d", "e"] {
        open_waiting(&mut model, &mut scene, id);
        scene.sync(&model);
    }
    settle(&mut scene);

    arrive(&mut model, &mut scene, "e");
    scene.sync(&model);
    settle(&mut scene);
    let e = drawn(&scene, "e");
    assert!(
        e.x >= 0.0 && e.x + e.w <= 1920.0,
        "e has the focus and a buffer: in view at {e:?}"
    );
    assert!(scene.visual("e").unwrap().visible);
    assert_eq!(e.w, place(&model, "e").rect.w);
    // d still waits, so no room is made for it between c and e.
    let (b, c) = (drawn(&scene, "b"), drawn(&scene, "c"));
    assert_eq!(e.x - (c.x + c.w), c.x - (b.x + b.w));
}
