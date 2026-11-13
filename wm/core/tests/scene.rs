mod common;

use std::collections::BTreeMap;

use common::*;
use formalwm_core::{
    geom::Rect,
    model::Placement,
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
