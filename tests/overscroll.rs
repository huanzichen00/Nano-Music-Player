use slint::ComponentHandle;
use std::time::Duration;

slint::slint! {
    import { ElasticAxis, ElasticFlickable } from "../ui/overscroll.slint";
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
        public function reset(value: length) { axis.reset(value); }
        axis := ElasticAxis { minimum: root.minimum; extent: root.extent; enabled: root.enabled; }
    }
}

fn tick(ms: u64) {
    // Pump each frame: advancing mock time by one large interval invokes a
    // repeating Timer once, unlike the live event loop.
    for _ in 0..ms.div_ceil(16) {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
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
    ui.invoke_drag(-10.0);
    assert!(ui.get_position() < rebound);
    ui.invoke_release();
    tick(2000);
    assert_eq!(ui.get_position(), 0.0);
    assert!(!ui.get_active());

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
