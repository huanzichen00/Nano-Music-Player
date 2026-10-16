//! Development comparison of the actual production ElasticAxis and its equations.
//! The 16ms Slint timer is unchanged: a 120/144Hz pump does not imply physics
//! updates at that rate. Mock time has millisecond precision, so nominal frame
//! deadlines are rounded cumulatively instead of truncating every interval.
use slint::ComponentHandle;
use std::{fmt::Write, time::Duration};

slint::slint! {
    import { ElasticAxis } from "../ui/overscroll.slint";
    import { ElasticPhysics, MotionState } from "../ui/overscroll-physics.slint";

    export component PhysicsComparison inherits Window {
        width: 240px; height: 300px;
        in property <int> configuration: 0;
        in property <length> extent: 300px;
        in property <length> minimum: -500px;
        out property <length> position: axis.position;
        out property <length> overshoot: axis.overshoot;
        out property <length> bounded: axis.bounded-position;
        out property <float> velocity: axis.motion-velocity;
        out property <bool> active: axis.active;
        public function reset(position: length) { axis.reset(position); }
        public function begin() { axis.begin(); }
        public function drag(delta: length) { axis.drag(delta); }
        public function release() { axis.release(); }
        public function wheel(delta: length) { axis.wheel(delta); }
        public pure function rubber(distance: length) -> length {
            return physics.rubber(distance, max(1px, root.extent * 0.32));
        }
        public pure function uncompress(distance: length) -> length {
            return physics.uncompress(distance, max(1px, root.extent * 0.32));
        }
        public pure function spring(offset: float, velocity: float, dt: float) -> MotionState {
            return physics.spring(offset, velocity, dt);
        }
        physics := ElasticPhysics { configuration: root.configuration; }
        axis := ElasticAxis {
            physics-configuration: root.configuration;
            extent: root.extent; minimum: root.minimum;
        }
    }
}

fn pump(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

// Slint Timer.start/stop update a property whose change handler registers or
// stops the runtime timer. A mock-time pump fires existing timers BEFORE those
// handlers. Flush each procedural input at the same timestamp, as the real
// event loop would, so the following frame cannot run a stale return callback
// during a held drag or start measuring before the release timer is armed.
fn reset(ui: &PhysicsComparison, position: f32) {
    ui.invoke_reset(position);
    pump(0);
}

fn begin(ui: &PhysicsComparison) {
    ui.invoke_begin();
    pump(0);
}

fn drag(ui: &PhysicsComparison, delta: f32) {
    ui.invoke_drag(delta);
    pump(0);
}

fn release(ui: &PhysicsComparison) {
    ui.invoke_release();
    pump(0);
}

fn harness(configuration: i32, extent: f32) -> PhysicsComparison {
    let ui = PhysicsComparison::new().unwrap();
    ui.set_configuration(configuration);
    ui.set_extent(extent);
    ui.show().unwrap();
    pump(0);
    ui
}

#[derive(Clone, Copy)]
enum Cadence {
    Hz(u64),
    Irregular,
    Delayed,
}

impl Cadence {
    fn name(self) -> String {
        match self {
            Self::Hz(hz) => format!("{hz}hz"),
            Self::Irregular => "irregular".into(),
            Self::Delayed => "delayed150ms".into(),
        }
    }

    fn interval(self, frame: u64) -> u64 {
        match self {
            Self::Hz(hz) => {
                let deadline = |n| (n * 1_000 + hz / 2) / hz;
                deadline(frame + 1) - deadline(frame)
            }
            Self::Irregular => [4, 25, 7, 12, 40, 9, 21][frame as usize % 7],
            Self::Delayed => {
                if frame == 0 {
                    150
                } else {
                    [8, 16, 33, 7, 16][(frame - 1) as usize % 5]
                }
            }
        }
    }
}

const CADENCES: [Cadence; 5] = [
    Cadence::Hz(60),
    Cadence::Hz(120),
    Cadence::Hz(144),
    Cadence::Irregular,
    Cadence::Delayed,
];

#[test]
fn resistance_is_monotone_invertible_and_scale_consistent() {
    i_slint_backend_testing::init_no_event_loop();
    for configuration in 0..=2 {
        for extent in [120.0, 240.0, 300.0, 600.0] {
            let ui = harness(configuration, extent);
            let mut previous = 0.0;
            let mut previous_step = f32::INFINITY;
            for step in 1..=100 {
                let distance = extent * step as f32 / 50.0;
                let compressed = ui.invoke_rubber(distance);
                let increment = compressed - previous;
                assert!(compressed > previous);
                assert!(increment <= previous_step + 0.0001);
                assert!((ui.invoke_rubber(-distance) + compressed).abs() < 0.0001);
                assert!((ui.invoke_uncompress(compressed) - distance).abs() < 0.005);
                previous = compressed;
                previous_step = increment;
            }
            let cap = extent * if configuration == 2 { 0.40 } else { 0.32 };
            let near_limit = ui.invoke_rubber(1_000_000.0);
            assert!(near_limit.is_finite() && near_limit <= cap + 0.0001);
            assert!(ui.invoke_uncompress(near_limit).is_finite());
            assert_eq!(ui.invoke_rubber(0.0), 0.0);
            let tiny = ui.invoke_rubber(0.0001);
            assert!(tiny > 0.0 && tiny < 0.0001, "continuous boundary entry");
            // Display scaling should not alter the normalized displacement.
            let normalized = ui.invoke_rubber(extent * 0.5) / extent;
            ui.set_extent(extent * 1.6);
            pump(0);
            assert!((ui.invoke_rubber(extent * 0.8) / (extent * 1.6) - normalized).abs() < 0.00001);
            ui.hide().unwrap();
        }
    }
}

#[test]
fn exact_candidates_preserve_initial_velocity_and_compose_across_frames() {
    i_slint_backend_testing::init_no_event_loop();
    for configuration in [1, 2] {
        let ui = harness(configuration, 300.0);
        for velocity in [-2_000.0, 0.0, 2_000.0] {
            let unchanged = ui.invoke_spring(40.0, velocity, 0.0);
            assert_eq!(unchanged.offset, 40.0);
            assert_eq!(unchanged.velocity, velocity);
            let tiny = ui.invoke_spring(40.0, velocity, 0.00001);
            assert!(((tiny.offset - 40.0) / 0.00001 - velocity).abs() < 10.0);
            // Both paths call the SAME production solver; no Rust physics model.
            let whole = ui.invoke_spring(40.0, velocity, 0.15);
            let mut split = unchanged;
            for interval in [0.007, 0.013, 0.017, 0.033, 0.08] {
                split = ui.invoke_spring(split.offset, split.velocity, interval);
            }
            assert!((split.offset - whole.offset).abs() < 0.00001);
            assert!((split.velocity - whole.velocity).abs() < 0.0001);
            assert!(whole.offset.is_finite() && whole.velocity.is_finite());
        }
        ui.hide().unwrap();
    }
}

#[test]
fn candidates_keep_real_axis_reversals_release_and_delayed_recovery() {
    i_slint_backend_testing::init_no_event_loop();
    for configuration in 0..=2 {
        for cadence in CADENCES {
            let ui = harness(configuration, 300.0);
            for (boundary, direction) in [(0.0, 1.0), (-500.0, -1.0)] {
                reset(&ui, boundary);
                begin(&ui);
                let mut previous = 0.0;
                for _ in 0..8 {
                    pump(16);
                    drag(&ui, 20.0 * direction);
                    let offset = ui.get_overshoot().abs();
                    assert!(offset > previous);
                    assert_eq!(ui.get_bounded(), boundary);
                    previous = offset;
                }
                // Moving release retains the production pointer velocity estimate.
                let incoming_velocity = ui.get_velocity();
                assert!((ui.get_overshoot() - ui.invoke_rubber(160.0 * direction)).abs() < 0.002);
                release(&ui);
                assert_eq!(ui.get_velocity(), incoming_velocity);
                let mut elapsed = 0;
                for frame in 0..100 {
                    let interval = cadence.interval(frame);
                    elapsed += interval;
                    pump(interval);
                    assert!(ui.get_position().is_finite());
                    assert!(ui.get_velocity().is_finite());
                    assert_eq!(ui.get_bounded(), boundary);
                    if !ui.get_active() {
                        break;
                    }
                }
                // These schedule-specific criteria supplement A's unchanged
                // 128ms regular-frame assertion. A's 32ms clamp discards most
                // of the delayed frame; analytic B/C recover on that callback.
                let deadline = match (configuration, cadence) {
                    (0, Cadence::Delayed) => 320,
                    (_, Cadence::Delayed) => 160,
                    _ => 250,
                };
                assert!(
                    !ui.get_active() && elapsed <= deadline,
                    "configuration={configuration}, cadence={}, elapsed={elapsed}, deadline={deadline}",
                    cadence.name()
                );
                assert_eq!(ui.get_position(), boundary);

                // A held pull and rapid equal inward/outward input never invoke
                // the spring, lose the inverse coordinate, or activate inertia.
                begin(&ui);
                drag(&ui, 160.0 * direction);
                let held = ui.get_position();
                pump(500);
                assert_eq!(ui.get_position(), held);
                for _ in 0..20 {
                    drag(&ui, -8.0 * direction);
                    assert!((ui.get_position() - boundary).abs() < (held - boundary).abs());
                    drag(&ui, 8.0 * direction);
                    assert!((ui.get_position() - held).abs() < 0.002);
                }
                reset(&ui, boundary);
                pump(200);
                assert!(!ui.get_active());
            }
            ui.hide().unwrap();
        }
    }
}

#[derive(Default)]
struct Measurement {
    initial: f32,
    release_velocity: f32,
    first_time_ms: u64,
    first_offset: f32,
    first_velocity: f32,
    peak: f32,
    peak_time_ms: u64,
    peak_velocity: f32,
    largest_step: f32,
    settle_ms: u64,
}

fn measure(ui: &PhysicsComparison, cadence: Cadence, moving: bool) -> Measurement {
    reset(ui, 0.0);
    begin(ui);
    if moving {
        for _ in 0..8 {
            pump(16);
            drag(ui, 20.0);
        }
    } else {
        drag(ui, 160.0);
        pump(128);
    }
    let expected_initial = ui.invoke_rubber(160.0);
    assert!(
        (ui.get_overshoot() - expected_initial).abs() < 0.002,
        "each scenario must start from the same 160px input: actual={}, expected={expected_initial}",
        ui.get_overshoot()
    );
    let incoming_velocity = ui.get_velocity();
    release(ui);
    assert!((ui.get_overshoot() - expected_initial).abs() < 0.002);
    assert_eq!(
        ui.get_velocity(),
        if moving { incoming_velocity } else { 0.0 }
    );
    let mut result = Measurement {
        initial: ui.get_overshoot(),
        release_velocity: ui.get_velocity(),
        peak: ui.get_overshoot(),
        ..Default::default()
    };
    let mut elapsed = 0;
    let mut previous = result.initial;
    for frame in 0..300 {
        let interval = cadence.interval(frame);
        elapsed += interval;
        pump(interval);
        let offset = ui.get_overshoot();
        let velocity = ui.get_velocity();
        if result.first_time_ms == 0 && offset != result.initial {
            result.first_time_ms = elapsed;
            result.first_offset = offset;
            result.first_velocity = velocity;
        }
        if offset > result.peak {
            result.peak = offset;
            result.peak_time_ms = elapsed;
        }
        result.peak_velocity = result.peak_velocity.max(velocity.abs());
        result.largest_step = result.largest_step.max((offset - previous).abs());
        previous = offset;
        if !ui.get_active() {
            result.settle_ms = elapsed;
            break;
        }
    }
    assert!(
        result.settle_ms > 0,
        "comparison must settle and stop its timer"
    );
    result
}

#[test]
#[ignore = "development-only numerical A/B/C comparison; run with --ignored --nocapture"]
fn export_production_axis_comparison_csv() {
    i_slint_backend_testing::init_no_event_loop();
    let mut csv = String::from(
        "configuration,cadence,extent_px,release,initial_px,release_velocity_px_s,first_time_ms,first_px,first_velocity_px_s,peak_px,peak_time_ms,peak_speed_px_s,largest_step_px,settle_ms\n",
    );
    for configuration in 0..=2 {
        for extent in [120.0, 240.0, 300.0, 600.0] {
            let ui = harness(configuration, extent);
            for cadence in CADENCES {
                for moving in [false, true] {
                    let m = measure(&ui, cadence, moving);
                    writeln!(csv, "{configuration},{},{extent:.0},{},{:.6},{:.6},{},{:.6},{:.6},{:.6},{},{:.6},{:.6},{}", cadence.name(), if moving { "moving" } else { "paused" }, m.initial, m.release_velocity, m.first_time_ms, m.first_offset, m.first_velocity, m.peak, m.peak_time_ms, m.peak_velocity, m.largest_step, m.settle_ms).unwrap();
                }
            }
            ui.hide().unwrap();
        }
    }
    print!("{csv}");
    if let Some(path) = std::env::var_os("NANO_OVERSCROLL_CSV") {
        std::fs::write(path, csv).unwrap();
    }
}
