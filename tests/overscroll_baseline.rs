//! Baseline measurements through the unmodified production axis.
use slint::ComponentHandle;
use std::time::Duration;

slint::slint! {
    import { ElasticAxis } from "../ui/overscroll.slint";
    export component Baseline inherits Window {
        out property <length> position: axis.position;
        out property <bool> active: axis.active;
        public function begin() { axis.begin(); }
        public function drag(delta: length) { axis.drag(delta); }
        public function release() { axis.release(); }
        public function reset() { axis.reset(0px); }
        axis := ElasticAxis { extent: 300px; minimum: -500px; physics-configuration: 0; }
    }
}

#[test]
fn record_baseline() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = Baseline::new().unwrap();
    ui.show().unwrap();
    for hz in [60, 120, 144] {
        ui.invoke_reset();
        ui.invoke_begin();
        ui.invoke_drag(160.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(128));
        let held = ui.get_position();
        assert!((held - 45.913044).abs() < 0.0001);
        ui.invoke_release();
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let step = Duration::from_nanos(1_000_000_000 / hz);
        let mut elapsed = Duration::ZERO;
        let mut first = None;
        while ui.get_active() && elapsed < Duration::from_secs(2) {
            i_slint_backend_testing::mock_elapsed_time(step);
            elapsed += step;
            if first.is_none() && ui.get_position() != held {
                first = Some((elapsed.as_secs_f64() * 1000.0, ui.get_position()));
            }
        }
        println!(
            "A,{hz}Hz,held={held:.6},first={first:?},settle_ms={:.3}",
            elapsed.as_secs_f64() * 1000.0
        );
        assert_eq!(ui.get_position(), 0.0);
        assert!(!ui.get_active());
        let (expected_first, expected_offset, expected_settle) = if hz == 144 {
            (20.833332, 15.63831, 125.0)
        } else {
            (16.666666, 17.01238, 116.666662)
        };
        let (first_time, first_offset) = first.unwrap();
        assert!((first_time - expected_first).abs() < 0.001);
        assert!((first_offset - expected_offset).abs() < 0.0001);
        assert!((elapsed.as_secs_f64() * 1000.0 - expected_settle).abs() < 0.001);
    }
    ui.invoke_reset();
    ui.invoke_begin();
    ui.invoke_drag(160.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(128));
    ui.invoke_release();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
    println!(
        "A,delayed150ms,position={:.6},active={}",
        ui.get_position(),
        ui.get_active()
    );
    assert!((ui.get_position() - 9.969651).abs() < 0.0001);
    assert!(ui.get_active());
}
