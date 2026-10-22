//! Artist-photo presentation; the library worker owns all disk and HTTP work.
use super::*;
use crate::library::artist_images::{
    ArtistRequest, Event, ImageCredit, JobState, normalized_artist_key,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) struct ArtistPhoto {
    pub image: slint::Image,
    pub credit: ImageCredit,
}

#[derive(Default)]
pub(super) struct ArtistPhotoCache {
    images: HashMap<String, ArtistPhoto>,
    order: VecDeque<String>,
    requested: HashSet<String>,
}

impl ArtistPhotoCache {
    fn insert(&mut self, key: String, pixels: image::RgbaImage, credit: ImageCredit) {
        let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
        );
        self.requested.remove(&key);
        self.order.retain(|k| k != &key);
        self.order.push_back(key.clone());
        self.images.insert(
            key,
            ArtistPhoto {
                image: slint::Image::from_rgba8(pixels),
                credit,
            },
        );
        while self.images.len() > 48
            || self
                .images
                .values()
                .map(|p| {
                    let size = p.image.size();
                    u64::from(size.width) * u64::from(size.height) * 4
                })
                .sum::<u64>()
                > 32 * 1024 * 1024
        {
            if let Some(key) = self.order.pop_front() {
                self.images.remove(&key);
            } else {
                break;
            }
        }
    }

    fn get(&self, artist: &str) -> Option<&ArtistPhoto> {
        self.images.get(&normalized_artist_key(artist))
    }
}

impl Controller {
    /// Explicit runtime opt-in keeps reusable/headless controllers offline.
    /// The desktop entry point enables this once; importing then only enqueues.
    pub fn enable_artist_images(&mut self) {
        self.artist_images_enabled = true;
        self.enqueue_library_artist_images();
    }

    pub(super) fn enqueue_library_artist_images(&self) {
        if !self.artist_images_enabled {
            return;
        }
        let mut artists: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
        for track in self
            .state
            .tracks
            .iter()
            .filter(|t| self.state.track_available(t))
        {
            let key = normalized_artist_key(&track.artist);
            if key.is_empty() || matches!(key.as_str(), "unknown artist" | "various artists") {
                continue;
            }
            let entry = artists
                .entry(key)
                .or_insert_with(|| (track.artist.clone(), BTreeSet::new()));
            if !track.album.trim().is_empty() {
                entry.1.insert(track.album.clone());
            }
        }
        self.artist_images.enqueue(
            artists
                .into_values()
                .map(|(artist, albums)| ArtistRequest {
                    artist,
                    albums: albums.into_iter().take(8).collect(),
                })
                .collect(),
        );
    }

    pub(super) fn poll_artist_images(&mut self) {
        for event in self.artist_images.poll() {
            self.apply_artist_image_event(event);
        }
    }

    fn apply_artist_image_event(&mut self, event: Event) {
        match event {
            Event::Queue(queue) => {
                if queue != self.state.artist_image_queue {
                    self.state.artist_image_queue = queue;
                    self.state.clamp_statistics_selection();
                    self.rows_dirty |= self.state.navigation.current().screen.is_statistics();
                }
            }
            Event::Image {
                key,
                pixels,
                credit,
            } => {
                self.artist_photos.insert(key, pixels, credit);
                self.rows_dirty |= self.state.navigation.current().screen.is_statistics();
            }
            Event::Error(error) => {
                // A photo service failure never replaces playback status.
                self.state.artist_image_error = Some(error);
                self.rows_dirty |= self.state.navigation.current().screen
                    == Screen::Statistics(StatisticsPage::ArtistImages);
            }
            Event::Recovered => {
                if self.state.artist_image_error.take().is_some() {
                    self.rows_dirty |= self.state.navigation.current().screen
                        == Screen::Statistics(StatisticsPage::ArtistImages);
                }
            }
        }
    }

    pub(super) fn artist_photo_for_target(&self, target: &Target) -> Option<&ArtistPhoto> {
        if let Target::Screen(Screen::StatisticsDetail { group, .. }) = target
            && group.artist.is_none()
        {
            return self.artist_photos.get(&group.name);
        }
        None
    }

    pub(super) fn current_artist_photo(&self) -> Option<&ArtistPhoto> {
        if let Screen::StatisticsDetail { group, .. } = &self.state.navigation.current().screen
            && group.artist.is_none()
        {
            return self.artist_photos.get(&group.name);
        }
        None
    }

    pub(super) fn request_visible_artist_photos(&mut self, ui: &AppWindow, screen: &Screen) {
        if !screen.is_statistics() {
            return;
        }
        let mut names = BTreeSet::new();
        if let Screen::StatisticsDetail { group, .. } = screen
            && group.artist.is_none()
        {
            names.insert(group.name.clone());
        }
        let height = ui.get_list_row_height().max(1.0);
        let start = ((-ui.get_list_scroll_y()).max(0.0) / height).floor() as usize;
        let count = ((ui.get_list_viewport_height() - ui.get_list_header_height()).max(0.0)
            / height)
            .ceil() as usize
            + 2;
        for e in self
            .state
            .statistics_entries()
            .iter()
            .skip(start.saturating_sub(1))
            .take(count)
        {
            if let Target::Screen(Screen::StatisticsDetail { group, .. }) = &e.row.target
                && group.artist.is_none()
            {
                names.insert(group.name.clone());
            }
        }
        let mut requests = Vec::new();
        for name in names {
            let key = normalized_artist_key(&name);
            if !self.artist_photos.images.contains_key(&key)
                && !self.artist_photos.requested.contains(&key)
                && self
                    .state
                    .artist_image_queue
                    .jobs
                    .iter()
                    .any(|j| j.key == key && j.state == JobState::Ready)
            {
                self.artist_photos.requested.insert(key);
                requests.push(name);
            }
        }
        if !requests.is_empty() {
            self.artist_images.request_cached(requests);
        }
    }

    pub(super) fn open_artist_photo_credits(&mut self) {
        let Some(photo) = self.current_artist_photo() else {
            return;
        };
        let url = &photo.credit.description_url;
        if !valid_credit_url(url) {
            return;
        }
        #[cfg(target_os = "macos")]
        let command = ("open", Vec::<&str>::new());
        #[cfg(target_os = "windows")]
        let command = ("rundll32.exe", vec!["url.dll,FileProtocolHandler"]);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let command = ("xdg-open", Vec::<&str>::new());
        if std::process::Command::new(command.0)
            .args(command.1)
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_err()
        {
            self.state.artist_image_error = Some("Unable to open photo credits".into());
        }
    }
}

fn valid_credit_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("commons.wikimedia.org")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.path().starts_with("/wiki/File:")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::navigation::{StatisticsDetailPage, StatisticsGroup},
        library::{Track, statistics::StatisticsStore},
    };
    use std::{
        io::Cursor,
        sync::Arc,
        time::{Duration, Instant},
    };

    fn credit() -> ImageCredit {
        ImageCredit {
            author: "Photographer".into(),
            license: "CC BY-SA 4.0".into(),
            license_url: "https://creativecommons.org/licenses/by-sa/4.0/".into(),
            description_url: "https://commons.wikimedia.org/wiki/File:Artist.jpg".into(),
        }
    }

    #[test]
    fn credits_only_open_https_commons_file_pages() {
        assert!(valid_credit_url(&credit().description_url));
        for url in [
            "http://commons.wikimedia.org/wiki/File:Artist.jpg",
            "https://commons.wikimedia.org.evil.test/wiki/File:Artist.jpg",
            "https://user:password@commons.wikimedia.org/wiki/File:Artist.jpg",
            "https://commons.wikimedia.org:8443/wiki/File:Artist.jpg",
            "https://commons.wikimedia.org/wiki/Other",
            "file:///tmp/Artist.jpg",
        ] {
            assert!(!valid_credit_url(url), "{url}");
        }
    }

    #[test]
    fn portrait_cache_bounds_both_image_count_and_decoded_memory() {
        i_slint_backend_testing::init_no_event_loop();
        let mut cache = ArtistPhotoCache::default();
        for id in 0..49 {
            cache.insert(id.to_string(), image::RgbaImage::new(1, 1), credit());
        }
        assert_eq!(cache.images.len(), 48);
        assert!(cache.get("0").is_none());
        for id in 0..30 {
            cache.insert(
                format!("large{id}"),
                image::RgbaImage::new(640, 640),
                credit(),
            );
        }
        let bytes: u64 = cache
            .images
            .values()
            .map(|p| {
                let size = p.image.size();
                u64::from(size.width) * u64::from(size.height) * 4
            })
            .sum();
        assert!(bytes <= 32 * 1024 * 1024);
        assert!(cache.get("large29").is_some());
    }

    #[test]
    fn portraits_survive_late_cover_decodes_and_storage_recovery_preserves_playback() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        let mut cover = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(48, 48, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut cover, image::ImageFormat::Png)
            .unwrap();
        let path = directory.path().join("song.flac");
        controller.state.tracks.push(Track {
            path: path.clone(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            genre: String::new(),
            number: 1,
            duration: 120.0,
            artwork: cover.into_inner(),
            lyrics: Default::default(),
        });
        let store = StatisticsStore::open(&directory.path().join("fixture.sqlite3")).unwrap();
        store
            .record_play(&path, chrono::Utc::now().timestamp(), 30)
            .unwrap();
        controller.state.statistics.snapshot = Some(
            store
                .listening_statistics(
                    &[crate::library::statistics::StatisticsTrack {
                        path: path.clone(),
                        title: "Song".into(),
                        artist: "Artist".into(),
                        album: "Album".into(),
                    }],
                    Default::default(),
                    chrono::Local::now(),
                )
                .unwrap(),
        );
        let artists = Screen::Statistics(StatisticsPage::Artists);
        controller.state.navigation.open(artists.clone());
        controller.initialize(&ui);
        controller.apply_artist_image_event(Event::Image {
            key: normalized_artist_key("Artist"),
            pixels: image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 128, 255, 255])),
            credit: credit(),
        });
        controller.sync_statistics_view(&ui);
        let portrait = controller
            .artist_photos
            .get("ARTIST")
            .unwrap()
            .image
            .clone();
        assert_eq!(
            controller.statistics_items.row_data(0).unwrap().artwork,
            portrait
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            controller.refresh_thumbnails(&ui, &artists);
            if controller.thumbnails.cached(0).is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "offline cover decoder did not finish"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            controller.statistics_items.row_data(0).unwrap().artwork,
            portrait,
            "a late album cover must never replace an artist portrait"
        );
        let artist_group = StatisticsGroup {
            name: " Artist ".into(),
            artist: None,
            paths: Arc::new([path.clone()].into_iter().collect()),
        };
        controller.state.navigation.open(Screen::StatisticsDetail {
            group: artist_group.clone(),
            page: StatisticsDetailPage::Summary,
        });
        controller.sync_statistics_view(&ui);
        assert_eq!(ui.get_statistics_summary().detail_artwork, portrait);
        assert!(
            ui.get_statistics_summary()
                .photo_credit
                .contains("Photographer")
        );
        assert!(
            controller
                .artist_photo_for_target(&Target::Screen(Screen::StatisticsDetail {
                    group: StatisticsGroup {
                        name: "Artist".into(),
                        artist: Some("Album Artist".into()),
                        paths: artist_group.paths
                    },
                    page: StatisticsDetailPage::Summary,
                }))
                .is_none(),
            "albums retain their own covers even when the album name matches an artist"
        );
        controller.state.navigation.open(Screen::StatisticsDetail {
            group: StatisticsGroup {
                name: "Other Artist".into(),
                artist: None,
                paths: Arc::new([path].into_iter().collect()),
            },
            page: StatisticsDetailPage::Summary,
        });
        controller.sync_statistics_view(&ui);
        assert_eq!(ui.get_statistics_summary().photo_credit, "");
        assert_ne!(ui.get_statistics_summary().detail_artwork, portrait);

        controller
            .state
            .navigation
            .open(Screen::Statistics(StatisticsPage::ArtistImages));
        controller.state.status = PlaybackStatus::Playing;
        controller.state.position = 57.0;
        controller.state.notice = "Existing playback notice".into();
        controller.apply_artist_image_event(Event::Error("Cannot save artist image queue".into()));
        controller.apply_artist_image_event(Event::Queue(Default::default()));
        assert!(
            controller.state.artist_image_error.is_some(),
            "a queue publication is not evidence of disk recovery"
        );
        controller.apply_artist_image_event(Event::Recovered);
        assert!(controller.state.artist_image_error.is_none());
        assert_eq!(controller.state.notice, "Existing playback notice");
        assert_eq!(controller.state.status, PlaybackStatus::Playing);
        assert_eq!(controller.state.position, 57.0);
        assert!(
            !controller.artist_images_enabled,
            "test controllers stay offline"
        );
    }
}
