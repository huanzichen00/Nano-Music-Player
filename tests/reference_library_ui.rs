use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::{Filter, Group, Screen},
        state::Target,
    },
    library::{Track, lyrics::Lyrics, playlists::Playlist},
    ui::AppWindow,
};
use slint::{ComponentHandle, Model};
use std::{cell::RefCell, rc::Rc};
fn point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let s = ui.get_display_scale();
    let h = ui.get_content_height();
    slint::LogicalPosition::new(
        (size.width - 240. * s) / 2. + x * s,
        size.height - h + (h - 432. * s) / 2. + y * s,
    )
}
fn snapshot(ui: &AppWindow, name: &str) {
    let shot = ui.window().take_snapshot().unwrap();
    let dir = std::env::temp_dir().join("nano-qa");
    std::fs::create_dir_all(&dir).unwrap();
    image::save_buffer(
        dir.join(name),
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}
#[test]
fn photo_icons_toolbar_live_title_and_black_rating_screen_render() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    Settings {
        device_mode: false,
        smooth_slides: false,
        ..Default::default()
    }
    .save(&tmp.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let c = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    c.borrow_mut().state.merge_library(vec![Track {
        path: tmp.path().join("first.wav"),
        title: "Settings".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        genre: "Rock".into(),
        number: 1,
        duration: 60.,
        artwork: vec![],
        lyrics: Lyrics::default(),
    }]);
    connect(&ui, &c);
    c.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    {
        let mut c = c.borrow_mut();
        c.state.start(0);
        c.state.navigation.open(Screen::Music);
        c.execute(&ui, Action::FinishSearchEditing);
    }
    assert!(ui.get_music_list());
    assert_eq!(ui.get_rows().row_data(0).unwrap().subtitle, "Settings");
    assert!(!ui.get_rows().row_data(0).unwrap().chevron);
    assert_eq!(
        (0..7)
            .map(|i| ui.get_rows().row_data(i).unwrap().icon)
            .collect::<Vec<_>>(),
        [8, 1, 2, 3, 4, 5, 6]
    );
    snapshot(&ui, "reference-music.png");
    {
        let mut c = c.borrow_mut();
        c.state.tracks[0].title = "Now playing title".into();
        c.state.navigation.open(Screen::Groups(Group::Albums));
        c.execute(&ui, Action::FinishSearchEditing);
    }
    assert!(ui.get_rows().row_data(0).unwrap().show_artwork);
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "All Songs");
    assert!(!ui.get_shuffle_list());
    snapshot(&ui, "reference-albums.png");
    {
        let mut c = c.borrow_mut();
        let path = c.state.tracks[0].path.clone();
        c.state.playlists.push(Playlist {
            id: 7,
            name: "Workout Music".into(),
            tracks: vec![path],
        });
        c.state.navigation.open(Screen::Groups(Group::Playlists));
        c.execute(&ui, Action::FinishSearchEditing);
    }
    assert_eq!(ui.get_rows().row_data(2).unwrap().icon, 1);
    assert_eq!(ui.get_rows().row_data(3).unwrap().icon, 7);
    assert_eq!(ui.get_rows().row_data(4).unwrap().icon, 2);
    assert_eq!(ui.get_list_header_height(), 38. * ui.get_display_scale());
    snapshot(&ui, "reference-playlists.png");
    ui.invoke_item_clicked(1);
    assert!(ui.get_playlist_list_editing());
    ui.invoke_item_clicked(5);
    assert!(ui.get_playlist_editor_open());
    c.borrow_mut().playlist_command(&ui, "cancel", "");
    ui.set_playlist_editor_open(false);
    c.borrow_mut().execute(&ui, Action::Settings);
    assert_eq!(ui.get_rows().row_count(), 8);
    snapshot(&ui, "reference-settings.png");
    ui.invoke_item_clicked(0);
    snapshot(&ui, "reference-display-settings.png");
    {
        let mut c = c.borrow_mut();
        c.state.navigation.open(Screen::NowPlaying);
        c.execute(&ui, Action::TrackList);
    }
    assert!(ui.get_rating_visible());
    assert!(!ui.get_shuffle_list());
    assert_eq!(ui.get_list_header_height(), 48. * ui.get_display_scale());
    snapshot(&ui, "reference-rating-black.png");
    let shot = ui.window().take_snapshot().unwrap();
    let scale = ui.window().scale_factor();
    for (x, y) in [
        (10., 10.),
        (225., 46.),
        (10., 88.),
        (225., 137.),
        (225., 175.),
        (120., 390.),
    ] {
        let p = point(&ui, x, y);
        let offset = ((p.y * scale) as usize * shot.width() as usize + (p.x * scale) as usize) * 4;
        assert!(
            shot.as_bytes()[offset..offset + 3].iter().all(|v| *v <= 35),
            "rating background at {x},{y}: pixel {:?}, device {}",
            &shot.as_bytes()[offset..offset + 3],
            ui.get_device_mode()
        );
    }
    snapshot(&ui, "reference-rating-black.png");
    assert!(matches!(c.borrow().state.rows()[0].target, Target::Play(0)));
    assert!(matches!(
        c.borrow().state.navigation.current().screen,
        Screen::Songs(Filter::Album { .. })
    ));
    ui.hide().unwrap();
}
