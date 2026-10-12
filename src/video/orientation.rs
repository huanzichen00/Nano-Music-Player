//! Video orientation is temporary presentation state, independent of saved zoom.
pub const BODY_WIDTH: f32 = 296.0;
pub const BODY_HEIGHT: f32 = BODY_WIDTH * 76.5 / 39.6;

pub fn window_base(device_mode: bool, landscape: bool) -> (f32, f32) {
    let upright = if device_mode {
        (BODY_WIDTH, BODY_HEIGHT)
    } else {
        (240.0, 432.0)
    };
    if landscape {
        (upright.1, upright.0)
    } else {
        upright
    }
}

/// Persist upright dimensions even if the app closes during landscape playback.
/// Native/fallback menu height stays outside the rotated device content.
pub fn saved_size(device_mode: bool, zoom: f32, menu_height: f32) -> (f32, f32) {
    let (width, height) = window_base(device_mode, false);
    (width * zoom, height * zoom + menu_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_swaps_the_entire_body_and_keeps_saved_dimensions_upright() {
        let upright = window_base(true, false);
        let landscape = window_base(true, true);
        assert_eq!(landscape, (upright.1, upright.0));
        assert_eq!(window_base(false, true), (432.0, 240.0));
        assert_eq!(
            saved_size(true, 1.25, 20.0),
            (296.0 * 1.25, BODY_HEIGHT * 1.25 + 20.0)
        );
        assert_eq!(window_base(true, false), upright);
    }
}
