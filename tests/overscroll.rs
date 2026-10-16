use slint::ComponentHandle;
use std::time::Duration;

slint::slint! {
    import { ElasticAxis, ElasticFlickable, WheelGesture } from "../ui/overscroll.slint";
    export component ElasticHarness inherits Window {
        width: 240px; height: 300px;
        in property <length> minimum: -500px;
        in property <length> extent: 300px;
        in property <bool> enabled: true;
        out property <length> position: axis.position;
        out property <length> bounded: axis.bounded-position;
        out property <bool> active: axis.active;
        public function begin() { axis.begin(); }
        public function drag(delta: length) { axis.drag(delta); }
        public function release() { axis.release(); }
        public function wheel(delta: length) { axis.wheel(delta); }
        public function native-wheel-active(active: bool) { WheelGesture.active = active; }
        public function reset(value: length) { axis.reset(value); }
        axis := ElasticAxis { minimum: root.minimum; extent: root.extent; enabled: root.enabled; }
    }
    export component FlickHarness inherits Window {
        width: 240px; height: 300px;
        out property <length> overshoot: list.overshoot;
        out property <length> visual-position: list.visual-position;
        out property <bool> active: list.user-interacting;
        in-out property <length> scroll-y;
        in property <duration> follow-duration;
        in property <length> content-height: 600px;
        in property <length> viewport-height: 300px;
        in property <bool> enabled: true;
        in property <int> navigation-generation;
        public function native-wheel-active(active: bool) { WheelGesture.active = active; }
        list := ElasticFlickable {
            width: root.width; height: root.viewport-height;
            content-height: root.content-height; enabled: root.enabled;
            navigation-generation: root.navigation-generation;
            content-y <=> root.scroll-y; follow-duration: root.follow-duration;
            Rectangle { width: 240px; height: 600px; background: #fff; TouchArea {} }
        }
    }
}

#[test]
fn flickable_receives_live_drag() {
    use slint::platform::{PointerEventButton, WindowEvent};
    i_slint_backend_testing::init_no_event_loop();
    let ui = FlickHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: slint::LogicalPosition::new(100.0, 100.0),
        button: PointerEventButton::Left,
    });
    for y in [110.0, 120.0, 130.0, 140.0] {
        tick(16);
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(100.0, y),
        });
    }
    assert!(
        ui.get_overshoot() > 0.0,
        "live offset {}",
        ui.get_overshoot()
    );
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: slint::LogicalPosition::new(100.0, 140.0),
        button: PointerEventButton::Left,
    });
    tick(2000);
    ui.set_follow_duration(180);
    ui.set_scroll_y(-200.0);
    tick(16);
    tick(64);
    let before = ui.get_visual_position();
    assert!(before < 0.0 && before > -200.0);
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(100.0, 100.0),
        delta_x: 0.0,
        delta_y: -10.0,
    });
    tick(16);
    assert!(
        (ui.get_visual_position() - before + 10.0).abs() < 0.1,
        "input interrupts lyric follow at its rendered position: before={before}, after={}, canonical={}",
        ui.get_visual_position(),
        ui.get_scroll_y()
    );
}

fn tick(ms: u64) {
    // Pump each frame: advancing mock time by one large interval invokes a
    // repeating Timer once, unlike the live event loop.
    for _ in 0..ms.div_ceil(16) {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

#[test]
fn spring_return_settles_promptly_at_both_edges() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    for (boundary, delta) in [(0.0, 160.0), (-500.0, -160.0)] {
        ui.invoke_reset(boundary);
        ui.invoke_begin();
        ui.invoke_drag(delta);
        let held = ui.get_position();
        tick(128);
        assert_eq!(ui.get_position(), held, "a paused drag stays held");
        ui.invoke_release();
        tick(64);
        assert!(
            (ui.get_position() - boundary).abs() < (held - boundary).abs(),
            "return progresses smoothly"
        );
        assert!(ui.get_active(), "return is still animated after 64ms");
        tick(64);
        assert_eq!(ui.get_position(), boundary, "return settles within 128ms");
        assert!(!ui.get_active(), "settled motion releases its timer");

        ui.invoke_wheel(delta);
        // Process the wheel callback's timer bindings before measuring its deadline.
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let stretched = (ui.get_position() - boundary).abs();
        tick(48);
        assert!(
            (ui.get_position() - boundary).abs() < stretched,
            "phase-less wheel return starts within 48ms of the last delta"
        );
        tick(112);
        assert_eq!(ui.get_position(), boundary);
        assert!(!ui.get_active());
    }
}

#[test]
fn continuous_wheel_input_does_not_fight_the_spring() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    for cadence in [8, 16, 24, 32] {
        for (boundary, direction) in [(0.0, 1.0), (-500.0, -1.0)] {
            ui.invoke_reset(boundary);
            for _ in 0..30 {
                ui.invoke_wheel(30.0 * direction);
                i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
                let stretched = ui.get_position();
                for _ in 0..cadence / 8 {
                    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(8));
                    assert_eq!(
                        ui.get_position(),
                        stretched,
                        "spring must not pull against live wheel input at {cadence}ms spacing"
                    );
                }
                assert_eq!(ui.get_bounded(), boundary);
            }
            assert!(
                (ui.get_position() - boundary).abs() > 70.0,
                "a sustained pull can reach the full elastic range"
            );
            tick(160);
            assert_eq!(ui.get_position(), boundary);
            assert!(!ui.get_active());
        }
    }
}

#[test]
fn native_wheel_gesture_holds_until_end_and_then_returns_quickly() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    // The native observer runs before Slint forwards that same terminal wheel
    // event to TouchArea, including its zero or final nonzero delta.
    for terminal_delta in [0.0, 4.0] {
        ui.invoke_reset(0.0);
        ui.invoke_native_wheel_active(true);
        ui.invoke_wheel(160.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        ui.invoke_wheel(terminal_delta);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let released = ui.get_position();
        tick(48);
        assert!(
            ui.get_position() < released,
            "terminal wheel callback preserves the short release guard: delta={terminal_delta}"
        );
        tick(112);
        assert_eq!(ui.get_position(), 0.0);
        assert!(!ui.get_active());
    }
    for (boundary, direction) in [(0.0, 1.0), (-500.0, -1.0)] {
        ui.invoke_reset(boundary);
        ui.invoke_native_wheel_active(true);
        let mut previous = 0.0;
        for delta in [160.0, 8.0, 6.0, 4.0, 3.0, 2.0, 1.0, 0.8, 0.6, 0.4, 0.2] {
            ui.invoke_wheel(delta * direction);
            i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
            let stretched = (ui.get_position() - boundary).abs();
            assert!(stretched >= previous, "outward input remains monotone");
            tick(64);
            assert_eq!((ui.get_position() - boundary).abs(), stretched);
            assert_eq!(ui.get_bounded(), boundary);
            previous = stretched;
        }
        tick(500);
        assert_eq!(
            (ui.get_position() - boundary).abs(),
            previous,
            "a held trackpad stays stretched"
        );
        ui.invoke_wheel(-20.0 * direction);
        assert!(
            (ui.get_position() - boundary).abs() < previous,
            "inward reversal follows input immediately"
        );
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let released = (ui.get_position() - boundary).abs();
        tick(48);
        assert!(
            (ui.get_position() - boundary).abs() < released,
            "native gesture end starts the return after the short handoff guard"
        );
        tick(112);
        assert_eq!(ui.get_position(), boundary);
        assert!(!ui.get_active());

        // A new gesture interrupts a return from its current rendered position.
        ui.invoke_native_wheel_active(true);
        ui.invoke_wheel(160.0 * direction);
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        tick(32);
        ui.invoke_native_wheel_active(true);
        ui.invoke_wheel(10.0 * direction);
        let resumed = ui.get_position();
        tick(500);
        assert_eq!(ui.get_position(), resumed);
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        tick(160);
        assert_eq!(ui.get_position(), boundary);
        assert!(!ui.get_active());

        // Native finger-end is followed by momentum-start, often at zero delta.
        ui.invoke_native_wheel_active(true);
        ui.invoke_wheel(160.0 * direction);
        let before_handoff = ui.get_position();
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        ui.invoke_wheel(0.0);
        tick(16);
        ui.invoke_native_wheel_active(true);
        ui.invoke_wheel(0.0);
        tick(64);
        assert_eq!(
            ui.get_position(),
            before_handoff,
            "native momentum handoff cannot twitch the stretch"
        );
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        tick(160);
        assert_eq!(ui.get_position(), boundary);
        assert!(!ui.get_active());
    }

    // Disabling a viewport cancels a held native gesture without a later replay.
    ui.invoke_reset(0.0);
    ui.invoke_native_wheel_active(true);
    ui.invoke_wheel(160.0);
    ui.set_enabled(false);
    tick(16);
    assert_eq!(ui.get_position(), 0.0);
    assert!(!ui.get_active());
    ui.invoke_native_wheel_active(false);
    ui.set_enabled(true);
    tick(128);
    assert_eq!(ui.get_position(), 0.0);
    assert!(!ui.get_active());
}

#[test]
fn elastic_axis_boundaries_resistance_momentum_and_interruptions() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    ui.invoke_begin();
    ui.invoke_drag(40.0);
    let first = ui.get_position();
    assert!(first > 0.0 && first < 40.0);
    tick(16);
    ui.invoke_drag(40.0);
    let second = ui.get_position();
    assert!(second > first && second - first < first);
    assert_eq!(ui.get_bounded(), 0.0);
    // A held gesture never springs back, even if the pointer pauses.
    tick(400);
    assert_eq!(ui.get_position(), second);
    ui.invoke_release();
    tick(64);
    assert!(ui.get_position() > 0.0 && ui.get_position() < second);
    let rebound = ui.get_position();
    ui.invoke_begin();
    assert_eq!(ui.get_position(), rebound);
    tick(16);
    // Stay outside the edge while interrupting the much shorter rebound.
    ui.invoke_drag(-rebound * 0.5);
    assert!(ui.get_position() > 0.0 && ui.get_position() < rebound);
    ui.invoke_release();
    tick(2000);
    assert_eq!(ui.get_position(), 0.0);
    assert!(!ui.get_active());

    // Large accumulated input near the resistance asymptote remains monotone.
    ui.invoke_begin();
    ui.invoke_drag(100_000.0);
    let large = ui.get_position();
    ui.invoke_drag(100.0);
    assert!(ui.get_position() >= large);
    ui.invoke_reset(0.0);

    ui.invoke_reset(-490.0);
    ui.invoke_begin();
    tick(16);
    ui.invoke_drag(-8.0);
    ui.invoke_release();
    tick(64);
    assert!(
        ui.get_position() < -500.0,
        "inertia crosses into overscroll"
    );
    tick(2000);
    assert_eq!(ui.get_position(), -500.0);
    for _ in 0..15 {
        ui.invoke_wheel(-25.0);
        tick(16);
        assert_eq!(ui.get_bounded(), -500.0);
    }
    assert!(ui.get_position() < -500.0);
    tick(2000);
    assert_eq!(ui.get_position(), -500.0);

    // Navigation/disable, content shrink and resize invalidate pending motion.
    ui.invoke_wheel(-100.0);
    ui.set_enabled(false);
    tick(2000);
    assert_eq!(ui.get_position(), -500.0);
    ui.set_enabled(true);
    ui.invoke_wheel(-60.0);
    ui.set_minimum(0.0);
    tick(16);
    assert_eq!(ui.get_position(), 0.0);
    ui.invoke_wheel(60.0);
    ui.set_extent(200.0);
    tick(16);
    assert_eq!(ui.get_position(), 0.0);
    tick(2000);
    assert!(!ui.get_active());
}

#[test]
fn resistance_is_partition_independent_and_reversible_across_the_boundary() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    for extent in [100.0, 300.0, 800.0] {
        ui.set_extent(extent);
        tick(16);
        for (boundary, direction) in [(0.0, 1.0), (-500.0, -1.0)] {
            ui.invoke_reset(boundary);
            ui.invoke_begin();
            ui.invoke_drag(160.0 * direction);
            let whole = ui.get_position();
            ui.invoke_reset(boundary);
            ui.invoke_begin();
            for _ in 0..160 {
                ui.invoke_drag(direction);
            }
            assert!(
                (ui.get_position() - whole).abs() < 0.01,
                "resistance must depend on accumulated distance, not event partitioning"
            );
            ui.invoke_drag(-160.0 * direction);
            assert!(
                (ui.get_position() - boundary).abs() < 0.01,
                "the inverse must undo a pull without retaining a visual offset"
            );

            // Start inside, cross the edge, then undo the same input. This
            // exercises the production inverse and bounded coordinates together.
            let inside = boundary - direction * 25.0;
            ui.invoke_reset(inside);
            ui.invoke_begin();
            ui.invoke_drag(120.0 * direction);
            assert!((ui.get_position() - boundary) * direction > 0.0);
            assert_eq!(ui.get_bounded(), boundary);
            ui.invoke_drag(-120.0 * direction);
            assert!((ui.get_position() - inside).abs() < 0.01);

            // Position is continuous at the edge, including tiny reversals.
            ui.invoke_reset(boundary - direction * 0.01);
            ui.invoke_begin();
            ui.invoke_drag(direction * 0.01);
            assert!((ui.get_position() - boundary).abs() < 0.0001);
            ui.invoke_drag(direction * 0.001);
            let outside = (ui.get_position() - boundary) * direction;
            assert!(outside > 0.0 && outside < 0.001);
            ui.invoke_drag(-direction * 0.002);
            let inside = (ui.get_position() - boundary) * direction;
            assert!(inside < 0.0 && inside > -0.002);
            ui.invoke_reset(boundary);
        }
    }
}

#[test]
fn equal_pulls_have_diminishing_gain_and_scale_with_the_viewport() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    let mut normalized: Option<f32> = None;
    for extent in [100.0, 300.0, 800.0] {
        ui.set_extent(extent);
        tick(16);
        ui.invoke_reset(0.0);
        ui.invoke_begin();
        let delta = extent * 0.1;
        let mut previous = 0.0;
        let mut previous_gain = delta;
        for _ in 0..20 {
            ui.invoke_drag(delta);
            let current = ui.get_position();
            let gain = current - previous;
            assert!(gain > 0.0 && gain < previous_gain);
            assert_eq!(ui.get_bounded(), 0.0);
            previous = current;
            previous_gain = gain;
        }
        let result = ui.get_position() / extent;
        if let Some(expected) = normalized {
            assert!((result - expected).abs() < 0.0001);
        }
        normalized = Some(result);
        ui.invoke_reset(0.0);
    }
}

#[test]
fn native_wheel_release_does_not_synthesize_inertia_inside_the_list() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    ui.invoke_reset(-250.0);
    ui.invoke_native_wheel_active(true);
    for (delta, interval) in [(-50.0, 8), (-20.0, 17), (-5.0, 27), (3.0, 6), (0.0, 200)] {
        ui.invoke_wheel(delta);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let held = ui.get_position();
        tick(interval);
        assert_eq!(ui.get_position(), held, "native events own all motion");
        assert!(ui.get_active());
    }
    let released = ui.get_position();
    ui.invoke_native_wheel_active(false);
    ui.invoke_wheel(0.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    tick(160);
    assert_eq!(ui.get_position(), released, "no second momentum animation");
    assert!(!ui.get_active());
    tick(1000);
    assert_eq!(ui.get_position(), released);
    assert!(!ui.get_active(), "the idle axis stays stopped");
}

#[test]
fn rapid_alternating_input_has_no_accumulated_drift_or_stale_rebound() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    for (boundary, direction) in [(0.0, 1.0), (-500.0, -1.0)] {
        ui.invoke_reset(boundary);
        ui.invoke_native_wheel_active(true);
        for _ in 0..60 {
            ui.invoke_wheel(20.0 * direction);
            let pulled = ui.get_position();
            assert!((pulled - boundary) * direction > 0.0);
            ui.invoke_wheel(-20.0 * direction);
            assert!((ui.get_position() - boundary).abs() < 0.001);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(7));
        }
        ui.invoke_native_wheel_active(false);
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        tick(160);
        // Opposite floating-point deltas can leave a tiny coordinate inside
        // the list; release preserves that bounded coordinate rather than
        // inventing a snap. Match the same inverse-roundtrip tolerance above.
        assert!((ui.get_position() - boundary).abs() < 0.001);
        assert!(!ui.get_active());

        for _ in 0..12 {
            ui.invoke_begin();
            ui.invoke_drag(80.0 * direction);
            tick(128); // release from rest, independent of the velocity filter
            ui.invoke_release();
            tick(32);
            let returning = ui.get_position();
            ui.invoke_begin();
            assert_eq!(ui.get_position(), returning);
            ui.invoke_drag(-10.0 * direction);
            let reversed = ui.get_position();
            assert!((reversed - returning) * direction < 0.0);
            tick(128);
            assert_eq!(
                ui.get_position(),
                reversed,
                "a held reversal stops the old spring"
            );
            ui.invoke_reset(boundary);
            tick(64);
            assert_eq!(ui.get_position(), boundary);
            assert!(!ui.get_active());
        }
    }
}

#[test]
fn irregular_and_delayed_frames_remain_finite_and_finish_without_crossing_the_edge() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ElasticHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    for (boundary, direction) in [(0.0, 1.0), (-500.0, -1.0)] {
        ui.invoke_reset(boundary);
        ui.invoke_begin();
        ui.invoke_drag(160.0 * direction);
        tick(128);
        ui.invoke_release();
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let mut previous = (ui.get_position() - boundary).abs();
        for interval in [7, 9, 8, 15, 23, 250, 4, 16, 48, 17, 100] {
            // One delayed frame intentionally delivers one timer callback.
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(interval));
            let position = ui.get_position();
            let offset = (position - boundary) * direction;
            assert!(position.is_finite());
            assert!(
                offset >= 0.0,
                "a rested rebound must not cross into the list"
            );
            assert!(offset <= previous + 0.0001);
            assert_eq!(ui.get_bounded(), boundary);
            previous = offset;
        }
        tick(2000);
        assert_eq!(ui.get_position(), boundary);
        assert!(!ui.get_active());
    }
}

#[test]
fn flickable_invalidation_during_a_native_hold_cannot_replay_motion() {
    use slint::platform::WindowEvent;
    i_slint_backend_testing::init_no_event_loop();
    let ui = FlickHarness::new().unwrap();
    ui.show().unwrap();
    tick(16);
    for invalidation in 0..4 {
        ui.set_enabled(true);
        ui.set_content_height(600.0);
        ui.set_scroll_y(0.0);
        tick(16);
        ui.invoke_native_wheel_active(true);
        ui.window().dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(100.0, 100.0),
            delta_x: 0.0,
            delta_y: 80.0,
        });
        tick(16);
        assert!(ui.get_overshoot() > 0.0);
        assert!(ui.get_active());
        match invalidation {
            0 => ui.set_navigation_generation(ui.get_navigation_generation() + 1),
            1 => ui.set_enabled(false),
            2 => ui.set_content_height(0.0),
            _ => ui.set_viewport_height(340.0),
        }
        tick(16);
        assert_eq!(ui.get_overshoot(), 0.0);
        assert_eq!(ui.get_scroll_y(), 0.0);
        assert!(!ui.get_active());
        ui.invoke_native_wheel_active(false);
        ui.set_enabled(true);
        tick(300);
        assert_eq!(ui.get_visual_position(), 0.0);
        assert!(
            !ui.get_active(),
            "an old native end cannot restart canceled motion"
        );
    }
}
