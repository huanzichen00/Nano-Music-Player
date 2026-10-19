#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        i18n::Language,
        navigation::{PodcastPage, Screen},
        state::{Effect, State},
    },
    audio::{PlaybackStatus, engine::AudioCommand},
    podcast::{Episode, Feed, Library},
    ui::AppWindow,
};
use slint::{ComponentHandle, Model};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

#[test]
fn podcast_transport_uses_its_own_position_and_ignores_music_modes() {
    let mut state = State::default();
    state.podcasts.library = Library {
        feeds: vec![Feed {
            id: 1,
            ..Feed::default()
        }],
        episodes: vec![Episode {
            id: 1,
            feed_id: 1,
            position: 100.0,
            duration: 3600.0,
            ..Episode::default()
        }],
    };
    state.podcasts.active = Some(1);
    state.podcasts.position = 100.0;
    state.podcasts.status = PlaybackStatus::Playing;
    state.podcasts.paused = false;
    state.position = 42.0;
    assert!(
        matches!(state.apply(Action::SeekForward).as_slice(),[Effect::Audio(AudioCommand::Seek(p))] if *p==130.0)
    );
    assert_eq!(state.position, 42.0);
    assert!(
        matches!(state.apply(Action::SeekBackward).as_slice(),[Effect::Audio(AudioCommand::Seek(p))] if *p==100.0)
    );
    assert!(state.apply(Action::ToggleShuffle).is_empty());
    assert!(!state.shuffle);
    assert!(matches!(
        state.apply(Action::TogglePlay).as_slice(),
        [Effect::Audio(AudioCommand::Pause(true))]
    ));
    state.apply(Action::NowPlaying);
    assert_eq!(
        state.navigation.current().screen,
        Screen::Podcasts(PodcastPage::Playing)
    );
    state.apply(Action::Back);
    assert_eq!(state.podcasts.active, Some(1));
    state.podcasts.library.episodes[0].local_path = Some("/podcasts/1.audio".into());
    state.podcasts.status = PlaybackStatus::Stopped;
    assert!(matches!(
        state.seek(50.0),
        Effect::Audio(AudioCommand::LoadAt {
            start: 50.0,
            paused: true,
            ..
        })
    ));
    assert!(state.podcasts.paused);
}

fn poll_until(c: &Rc<RefCell<Controller>>, ui: &AppWindow, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !predicate() {
        c.borrow_mut().poll(ui);
        assert!(Instant::now() < deadline, "podcast UI did not update");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn snapshot(ui: &AppWindow, name: &str) {
    let shot = ui.window().take_snapshot().unwrap();
    let directory = std::env::temp_dir().join("nano-qa");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(name),
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    use slint::platform::{PointerEventButton, WindowEvent};
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let position = slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        size.height - ui.get_content_height()
            + (ui.get_content_height() - 432.0 * scale) / 2.0
            + y * scale,
    );
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

#[test]
fn desktop_rss_preview_subscription_navigation_and_player_render() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/feed", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut data = [0u8; 4096];
        let _ = socket.read(&mut data).unwrap();
        let body = r#"<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel><title>Quiet Conversations</title><link>https://example.org</link><description>A show about music, ideas and everyday life.</description><itunes:author>Nano Studio</itunes:author><item><title>Finding a little space to listen</title><guid>episode-one</guid><description><![CDATA[<p>A thoughtful conversation about the music we return to.</p><p>In this episode we explore listening habits, favourite recordings, and why a familiar song can make a place feel like home.</p>]]></description><pubDate>Mon, 19 Oct 2026 08:00:00 +0800</pubDate><itunes:duration>1:02:03</itunes:duration><enclosure url="https://example.org/episode.mp3" type="audio/mpeg" length="100"/></item></channel></rss>"#;
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    Settings {
        device_mode: false,
        smooth_slides: false,
        language: Language::SimplifiedChinese,
        ..Settings::default()
    }
    .save(&dir.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let c = Rc::new(RefCell::new(Controller::new(dir.path().into())));
    connect(&ui, &c);
    c.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    {
        let mut c = c.borrow_mut();
        c.state.navigation.select(3);
        c.execute(&ui, Action::Activate);
    }
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Podcasts(PodcastPage::Home)
    );
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "添加订阅…");
    snapshot(&ui, "podcast-home-empty.png");
    ui.invoke_item_clicked(0);
    assert!(ui.get_podcast_subscribe_open());
    ui.set_podcast_url(url.into());
    ui.invoke_podcast_command("preview".into());
    poll_until(&c, &ui, || ui.get_podcast_preview_ready());
    assert_eq!(ui.get_podcast_preview_title(), "Quiet Conversations");
    assert!(c.borrow().state.podcasts.library.feeds.is_empty());
    snapshot(&ui, "podcast-subscription.png");
    ui.invoke_podcast_command("subscribe".into());
    assert!(!ui.get_podcast_subscribe_open());
    poll_until(&c, &ui, || {
        c.borrow().state.podcasts.library.feeds.len() == 1
    });
    server.join().unwrap();
    assert_eq!(ui.get_rows().row_count(), 5);
    snapshot(&ui, "podcast-home.png");
    assert!(ui.get_rows().row_data(0).unwrap().show_artwork);
    assert_eq!(ui.get_rows().row_data(0).unwrap().icon, 11);
    ui.invoke_item_clicked(0);
    assert_eq!(ui.get_rows().row_count(), 3);
    assert_eq!(ui.get_rows().row_data(0).unwrap().icon, 11);
    snapshot(&ui, "podcast-episodes.png");
    click(&ui, 224.0, 50.0);
    assert!(
        ui.get_podcast_detail(),
        "info click: screen={}, notice={}, downloads={:?}",
        ui.get_screen(),
        c.borrow().state.notice,
        c.borrow().state.podcasts.downloads
    );
    assert!(ui.get_podcast_description().contains("listening habits"));
    snapshot(&ui, "podcast-episode.png");
    {
        let mut c = c.borrow_mut();
        let id = c.state.podcasts.library.episodes[0].id;
        c.state.podcasts.active = Some(id);
        c.state.podcasts.position = 125.0;
        c.state.podcasts.paused = true;
        c.state.podcasts.status = PlaybackStatus::Paused;
        c.execute(&ui, Action::NowPlaying);
    }
    assert_eq!(ui.get_screen(), 10);
    assert_eq!(ui.get_elapsed(), "2:05");
    assert_eq!(ui.get_podcast_title(), "Finding a little space to listen");
    assert!(c.borrow().state.current.is_none());
    snapshot(&ui, "podcast-playing.png");
    click(&ui, 25.0, 13.0);
    assert!(ui.get_podcast_detail());
    c.borrow_mut().execute(&ui, Action::NowPlaying);
    click(&ui, 120.0, 200.0);
    assert!(ui.get_podcast_overlay());
    snapshot(&ui, "podcast-playing-controls.png");
    click(&ui, 30.0, 163.0);
    assert!(ui.get_podcast_repeat());
    click(&ui, 150.0, 163.0);
    assert_eq!(ui.get_podcast_speed(), "2X");
    click(&ui, 90.0, 163.0);
    assert_eq!(c.borrow().state.podcasts.position, 95.0);
    click(&ui, 210.0, 163.0);
    assert_eq!(ui.get_screen(), 1);
    c.borrow_mut().execute(&ui, Action::Home);
    assert!(c.borrow().state.podcasts.active.is_some());
    c.borrow_mut().save_settings(&ui);
    ui.hide().unwrap();
    drop(c);
}
