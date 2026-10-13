slint::include_modules!();

#[test]
fn directions_interruptions_and_motion_disable() {
    use slint::{ComponentHandle, platform::set_platform};

    set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();

    let ui = SmoothSlidesPrototype::new().unwrap();
    ui.show().unwrap();

    // Forward moves the old page left and brings the new page in from the right.
    ui.invoke_start_transition(1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(70));
    assert!(ui.get_outgoing_offset() < 0.0);
    assert!(ui.get_incoming_offset() > 0.0);
    assert_eq!(ui.get_page(), 1);
    let frame = ui.window().take_snapshot().unwrap();
    image::save_buffer(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prototype-forward-mid.png"),
        frame.as_bytes(),
        frame.width(),
        frame.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();

    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    assert!(!ui.get_moving());

    // Back reverses the travel direction after a completed forward transition.
    ui.invoke_start_transition(-1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(70));
    assert!(ui.get_outgoing_offset() > 0.0);
    assert!(ui.get_incoming_offset() < 0.0);
    assert_eq!(ui.get_page(), 2);

    // Rapid Back then Forward requests resolve to the latest destination.
    ui.invoke_start_transition(-1);
    ui.invoke_start_transition(1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(70));
    assert_eq!(ui.get_page(), 4);
    assert!(ui.get_outgoing_offset() < 0.0);
    assert!(ui.get_incoming_offset() > 0.0);

    // Disabling motion mid-flight commits the destination immediately and
    // advancing beyond the old animation deadline cannot move it again.
    ui.invoke_set_motion_enabled(false);
    assert!(!ui.get_moving());
    assert_eq!(ui.get_outgoing_offset(), 0.0);
    assert_eq!(ui.get_incoming_offset(), 0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(500));
    assert_eq!(ui.get_page(), 4);
    assert_eq!(ui.get_outgoing_offset(), 0.0);
    assert_eq!(ui.get_incoming_offset(), 0.0);

    // With motion disabled, new navigation is still immediate.
    ui.invoke_start_transition(-1);
    assert_eq!(ui.get_page(), 5);
    assert!(!ui.get_moving());

    let pixels = ui.window().take_snapshot().unwrap();
    image::save_buffer(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prototype-preview.png"),
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}
