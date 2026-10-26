//! State projection into the desktop Slint UI and its existing refresh markers.
//!
//! Query triggers, thumbnail requests, and navigation snapshots retain the
//! controller's established synchronization order and asynchronous ownership.
use super::*;
use crate::{
    app::navigation::SettingsSection,
    ui::{ListMetrics, ShortcutRow, StatisticsDay, StatisticsSummary},
};

impl Controller {
    pub(super) fn refresh_shortcuts(&self, ui: &AppWindow) {
        let mut reference = Vec::new();
        for category in [
            Category::Playback,
            Category::Navigation,
            Category::Application,
        ] {
            for binding in self
                .dispatcher
                .keymap
                .bindings
                .iter()
                .filter(|b| b.action.category() == category)
            {
                reference.push(ShortcutRow {
                    category: self.text(category.title()),
                    title: self.text(binding.action.title()),
                    keys: binding.label(cfg!(target_os = "macos")).into(),
                });
                if binding.action != Action::ToggleSubtitles
                    && self.dispatcher.dispatch(
                        &KeyPress {
                            key: binding.key.clone(),
                            modifiers: binding.modifiers,
                            repeat: false,
                        },
                        Context {
                            surface: crate::input::Surface::VideoPlaying,
                            text_input: false,
                            reference_open: false,
                            native_dialog: false,
                        },
                    ) == Dispatch::Action(Action::ToggleSubtitles)
                {
                    reference.push(ShortcutRow {
                        category: self.text(category.title()),
                        title: self.text(Action::ToggleSubtitles.title()),
                        keys: binding.label(cfg!(target_os = "macos")).into(),
                    });
                }
            }
        }
        ui.set_shortcuts(ModelRc::new(VecModel::from(reference)));
    }

    pub(super) fn text(&self, key: &str) -> slint::SharedString {
        translate(self.state.language, key).into()
    }

    pub(super) fn toggle_track_details(&mut self, ui: &AppWindow) {
        if ui.get_track_details_open() {
            ui.set_track_details_open(false);
            self.details_path = None;
            return;
        }
        let selected = self
            .state
            .rows()
            .get(self.state.navigation.selected())
            .cloned();
        let id = match &self.state.navigation.current().screen {
            Screen::Lyrics(id) | Screen::LyricsSearch(id) => Some(*id),
            Screen::NowPlaying => self.state.current,
            _ => selected
                .as_ref()
                .and_then(|row| match &row.target {
                    Target::Play(id) => Some(*id),
                    Target::Screen(Screen::Songs(Filter::Statistics { paths, .. }))
                        if paths.len() == 1 =>
                    {
                        self.state
                            .tracks
                            .iter()
                            .position(|t| paths.contains(&t.path))
                    }
                    _ => None,
                })
                .or(self
                    .state
                    .current
                    .filter(|_| !self.state.video_active && self.state.podcasts.active.is_none())),
        };
        let Some((id, track)) = id.and_then(|id| self.state.tracks.get(id).map(|t| (id, t))) else {
            self.state.notice = "Choose a song first".into();
            self.sync(ui);
            return;
        };
        self.details_path = Some(track.path.clone());
        ui.set_details_title(track.title.as_str().into());
        ui.set_details_artist(track.artist.as_str().into());
        ui.set_details_album(track.album.as_str().into());
        ui.set_details_artwork(self.thumbnails.cached(id).unwrap_or_default());
        ui.set_native_status_open(false);
        ui.set_track_details_open(true);
        ui.invoke_reset_details_scroll();
        ui.invoke_focus_screen();
    }

    pub(super) fn sync_statistics_view(&mut self, ui: &AppWindow) {
        use crate::app::statistics_projection::{bar_ratio, chart_ceiling, grouped_number};
        let entries = self.state.statistics_entries();
        let items = entries
            .into_iter()
            .map(|e| StatisticsItem {
                title: e.row.title.into(),
                subtitle: e.row.subtitle.into(),
                value: e.value.into(),
                rank: e.rank,
                ratio: e.ratio,
                icon: e.icon,
                chevron: matches!(
                    e.row.target,
                    Target::Screen(_)
                        | Target::StatisticsRange(_)
                        | Target::RefreshStatistics
                        | Target::PlayStatisticsGroup(_)
                        | Target::RetryArtistImages
                ),
                artwork: self
                    .artist_photo_for_target(&e.row.target)
                    .map(|p| p.image.clone())
                    .or_else(|| e.row.artwork_id.and_then(|id| self.thumbnails.cached(id)))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        self.statistics_items = Rc::new(VecModel::from(items));
        ui.set_statistics_items(self.statistics_items.clone().into());
        let snapshot = self.state.statistics.snapshot.as_ref();
        let maximum = chart_ceiling(snapshot.map_or(0, |s| {
            s.daily_plays.iter().map(|d| d.plays).max().unwrap_or(0)
        }));
        let days = snapshot.map_or_else(Vec::new, |s| {
            s.daily_plays
                .iter()
                .map(|day| StatisticsDay {
                    label: day.date.format("%Y-%m-%d").to_string().into(),
                    short_label: day.date.format("%m/%d").to_string().into(),
                    plays: grouped_number(day.plays).into(),
                    ratio: bar_ratio(day.plays, maximum),
                })
                .collect::<Vec<_>>()
        });
        ui.set_statistics_days(ModelRc::new(VecModel::from(days)));
        let (total, artists, albums, tracks) = self.state.statistics_counts();
        let detail = self.state.statistics_detail_info();
        let photo = self.current_artist_photo();
        ui.set_statistics_summary(StatisticsSummary {
            total: if snapshot.is_some() {
                grouped_number(total).into()
            } else {
                "-".into()
            },
            artists: grouped_number(artists).into(),
            albums: grouped_number(albums).into(),
            tracks: grouped_number(tracks).into(),
            range_label: self.text(self.state.statistics.range.label()),
            footer: self.state.statistics_note().into(),
            detail_title: detail.title.into(),
            detail_subtitle: detail.subtitle.into(),
            detail_artwork: photo
                .map(|p| p.image.clone())
                .or_else(|| detail.artwork_id.and_then(|id| self.thumbnails.cached(id)))
                .unwrap_or_default(),
            photo_credit: photo.map_or_else(
                || "".into(),
                |p| {
                    translate(self.state.language, "Photo: {0} · {1}")
                        .replace("{0}", &p.credit.author)
                        .replace("{1}", &p.credit.license)
                        .into()
                },
            ),
            photo_url: photo
                .map_or_else(|| "".into(), |p| p.credit.description_url.as_str().into()),
            chart_maximum: grouped_number(maximum).into(),
            chart_middle: if maximum == 1 {
                "0.5".into()
            } else {
                grouped_number(maximum / 2).into()
            },
        });
    }

    pub(super) fn sync(&mut self, ui: &AppWindow) {
        if self.state.presentation_mode
            && (self.state.video_active || self.state.podcasts.active.is_some())
        {
            self.set_presentation_mode(ui, false);
        }
        ui.set_presentation_mode(self.state.presentation_mode);
        self.sync_practice(ui);
        ui.set_library_busy(self.library_tasks.busy());
        if self.state.translation_target != self.settings.translation_target
            || self.state.translation_visible != self.lyrics.visible()
        {
            self.state.translation_target = self.settings.translation_target;
            self.state.translation_visible = self.lyrics.visible();
            self.rows_dirty = true;
        }
        ui.global::<I18n>()
            .set_language(self.state.language.index() as i32);
        let current_screen = self.state.navigation.current().screen.clone();
        let screen = &current_screen;
        let screen_changed = self.last_screen.as_ref() != Some(screen);
        if screen.is_statistics()
            && *screen != Screen::Statistics(StatisticsPage::ArtistImages)
            && (screen_changed || self.statistics.needs_query(self.state.statistics.range))
        {
            self.refresh_statistics(false);
        }
        if screen_changed && ui.get_track_details_open() {
            ui.set_track_details_open(false);
            self.details_path = None;
        }
        if screen.is_statistics() && screen.statistics_kind() == 5 {
            let minute = chrono::Utc::now().timestamp() / 60;
            if minute != self.last_statistics_minute {
                self.rows_dirty = true;
                self.last_statistics_minute = minute;
            }
        }
        let home_page = self.state.home_page as i32;
        if matches!(screen, Screen::Home) && ui.get_home_page() != home_page {
            ui.set_home_transition_direction(if home_page > ui.get_home_page() {
                1
            } else {
                -1
            });
        }
        let navigation_depth = self.state.navigation.depth();
        let transition_direction = if navigation_depth >= self.last_navigation_depth {
            1
        } else {
            -1
        };
        ui.set_smooth_slides(self.state.smooth_slides);
        if !self.state.smooth_slides {
            ui.invoke_cancel_screen_transition();
        }
        ui.set_transition_direction(transition_direction);
        if screen_changed && self.last_screen.is_some() {
            ui.set_previous_screen(ui.get_screen());
            ui.set_previous_screen_title(ui.get_screen_title());
            ui.set_previous_selected(ui.get_selected());
            ui.set_previous_home_page(ui.get_home_page());
            ui.set_previous_home_transition_direction(ui.get_home_transition_direction());
            ui.set_previous_rows(ui.get_rows());
            ui.set_previous_playlist_list(ui.get_playlist_list());
            ui.set_previous_music_list(ui.get_music_list());
            ui.set_previous_podcast_list(ui.get_podcast_list());
            ui.set_previous_playlist_content(ui.get_playlist_content());
            ui.set_previous_shuffle_list(ui.get_shuffle_list());
            ui.set_previous_statistics_page(ui.get_statistics_page());
            ui.set_previous_statistics_note(ui.get_statistics_note());
            ui.set_previous_statistics_kind(ui.get_statistics_kind());
            ui.set_previous_statistics_range_index(ui.get_statistics_range_index());
            ui.set_previous_statistics_items(ui.get_statistics_items());
            ui.set_previous_statistics_days(ui.get_statistics_days());
            ui.set_previous_statistics_summary(ui.get_statistics_summary());
            ui.set_previous_statistics_loading(ui.get_statistics_loading());
            ui.set_previous_statistics_failed(ui.get_statistics_failed());
            ui.set_previous_rating_visible(ui.get_rating_visible());
            ui.set_previous_rating_title(ui.get_rating_title());
            ui.set_previous_rating(ui.get_rating());
            ui.set_previous_list_scroll_y(ui.get_list_scroll_y());
            ui.set_previous_lyric_scroll_y(ui.get_lyric_scroll_y());
        }
        let landscape = self.state.landscape_video();
        ui.set_video_landscape(landscape);
        if landscape != self.last_landscape {
            self.last_landscape = landscape;
            self.resize(ui);
        }
        if self.last_video_generation != self.state.video_generation {
            self.last_video_generation = self.state.video_generation;
            self.subtitle_tracks.clear();
            self.subtitle_cursor = 0;
            ui.set_subtitle_menu_open(false);
            ui.set_subtitle_choices(ModelRc::new(VecModel::from(self.subtitle_choices())));
            ui.set_video_frame(slint::Image::default());
        }
        if screen != &Screen::VideoPlaying && ui.get_subtitle_menu_open() {
            ui.set_subtitle_menu_open(false);
        }
        ui.set_video_overlay(self.state.video_overlay);
        ui.set_video_title(
            self.state
                .video_current
                .and_then(|id| self.state.videos.get(id))
                .map_or("", |v| v.title.as_str())
                .into(),
        );
        ui.set_screen(match screen {
            Screen::Home => 0,
            Screen::NowPlaying => 2,
            Screen::Podcasts(crate::app::navigation::PodcastPage::Playing) => 10,
            Screen::VideoPlaying => 9,
            Screen::Search => 3,
            Screen::Placeholder(_) => 4,
            Screen::Settings | Screen::SettingsSection(_) => 5,
            Screen::Shortcuts => 6,
            Screen::Lyrics(_) | Screen::LyricsSearch(_) => 7,
            Screen::Languages | Screen::Fonts => 8,
            _ => 1,
        });
        ui.set_screen_title(match screen {
            Screen::Artist(_)
            | Screen::Songs(
                Filter::Album { .. }
                | Filter::Genre(_)
                | Filter::Playlist { .. }
                | Filter::Statistics { .. },
            ) => screen.title().into(),
            _ => self.text(screen.title()),
        });
        ui.set_statistics_page(screen.is_statistics());
        ui.set_statistics_note(if screen.is_statistics() {
            self.state.statistics_note().into()
        } else {
            "".into()
        });
        if screen.is_statistics() {
            ui.set_statistics_kind(screen.statistics_kind());
            ui.set_statistics_range_index(match self.state.statistics.range {
                crate::library::statistics::StatisticsRange::Last7Days => 0,
                crate::library::statistics::StatisticsRange::Last30Days => 1,
                crate::library::statistics::StatisticsRange::AllTime => 2,
            });
            let image_queue = *screen == Screen::Statistics(StatisticsPage::ArtistImages);
            ui.set_statistics_loading(self.state.statistics.loading && !image_queue);
            ui.set_statistics_failed(self.state.statistics.failed && !image_queue);
        }
        ui.set_settings_root(*screen == Screen::Settings);
        ui.set_device_mode(self.state.device_mode);
        let rating_target = self.state.rating_target();
        ui.set_rating_visible(rating_target.is_some());
        ui.set_rating_enabled(self.state.ratings_ready);
        ui.set_rating_target_path(
            rating_target
                .map_or_else(String::new, |t| t.path.to_string_lossy().into_owned())
                .into(),
        );
        ui.set_rating_title(rating_target.map_or("", |t| t.title.as_str()).into());
        ui.set_rating(
            rating_target
                .and_then(|t| self.state.ratings.get(&t.path))
                .copied()
                .unwrap_or(0) as i32,
        );
        ui.set_playlist_list(matches!(screen, Screen::Groups(Group::Playlists)));
        ui.set_playlist_list_editing(self.state.playlist_list_editing);
        ui.set_music_list(*screen == Screen::Music);
        ui.set_playlist_content(matches!(screen, Screen::Songs(Filter::Playlist { .. })));
        ui.set_shuffle_list(self.state.supports_list_shuffle());
        ui.set_ui_font_family(self.state.font.family().into());
        ui.set_keep_on_top(self.state.always_on_top);
        ui.set_selected(self.state.navigation.selected() as i32);
        ui.set_home_page(self.state.home_page as i32);
        ui.set_clock_text(chrono::Local::now().format("%-I:%M %p").to_string().into());
        self.sync_device_status(ui);
        // Publish only the active owner's state. Transient music/podcast flips
        // rebuild the transport buttons and discard an in-progress mouse click.
        ui.set_playing(if self.state.podcasts.active.is_some() {
            self.state.podcasts.status == PlaybackStatus::Playing
        } else if self.state.video_active {
            self.state.video_status == crate::video::Status::Playing
        } else {
            self.state.status == PlaybackStatus::Playing
        });
        ui.set_has_track(self.state.current.is_some());
        ui.set_track_path(
            self.state
                .current
                .and_then(|id| self.state.tracks.get(id))
                .map_or_else(String::new, |track| {
                    track.path.to_string_lossy().into_owned()
                })
                .into(),
        );
        ui.set_volume(self.state.volume);
        ui.set_muted(self.state.muted);
        ui.set_shuffle(self.state.shuffle);
        ui.set_repeat_mode(self.state.repeat.label().into());
        if self.last_notice != self.state.notice {
            self.last_notice.clone_from(&self.state.notice);
            self.notice_since = std::time::Instant::now();
        }
        ui.set_notice(
            if self.notice_since.elapsed().as_secs() < 3
                || self.state.importing
                || self.state.video_importing
                || self.state.romaji_generating.is_some()
            {
                i18n::notice(self.state.language, &self.state.notice).into()
            } else {
                "".into()
            },
        );
        ui.set_reference_open(self.state.reference_open);
        ui.set_lyrics_visible(self.state.lyrics_visible);
        let lookup_status = self
            .state
            .current
            .and_then(|id| self.state.tracks.get(id))
            .and_then(|track| self.state.lyrics_lookup_status.get(&track.path));
        let mut empty_lyrics_message = i18n::notice(
            self.state.language,
            lookup_status.map_or("No lyrics available", String::as_str),
        );
        let lookup_pending = matches!(
            lookup_status.map(String::as_str),
            Some("Searching online lyrics…" | "Saving online lyrics…")
        );
        if self.state.current.is_some()
            && !lookup_pending
            && self.dispatcher.keymap.bindings.iter().any(|binding| {
                binding.matches(&Key::Letter('f'), Modifiers::default())
                    && matches!(
                        binding.action,
                        Action::JumpToLetter | Action::FindOnlineLyrics
                    )
            })
        {
            empty_lyrics_message.push_str("\n\n");
            empty_lyrics_message.push_str(&self.text(if lookup_status.is_some() {
                "Press F to try again"
            } else {
                "Press F to find lyrics"
            }));
        }
        ui.set_empty_lyrics_message(empty_lyrics_message.into());
        ui.set_romaji_visible(self.state.romaji_visible);
        ui.set_translation_visible(self.lyrics.visible());
        ui.set_overlay_visible(self.state.overlay_visible);
        ui.set_track_count(if self.state.queue.order.is_empty() {
            "".into()
        } else {
            translate(self.state.language, "{0} of {1}")
                .replace("{0}", &(self.state.queue.position + 1).to_string())
                .replace("{1}", &self.state.queue.order.len().to_string())
                .into()
        });
        let duration = self.state.duration();
        ui.set_progress(if duration > 0.0 {
            (self.state.active_position() / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        });
        ui.set_elapsed(format_time(self.state.active_position()).into());
        ui.set_duration(format_time(duration).into());
        ui.set_remaining(
            format!(
                "−{}",
                format_time((duration - self.state.active_position()).max(0.0))
            )
            .into(),
        );
        if screen_changed
            || self.rows_dirty
            || self.last_query != self.state.query
            || (matches!(
                screen,
                Screen::Music | Screen::SettingsSection(SettingsSection::Translation)
            ) && self.last_track != self.state.current)
        {
            let metadata_rows = matches!(
                screen,
                Screen::Groups(Group::Artists | Group::Albums | Group::Genres)
                    | Screen::Artist(_)
                    | Screen::Songs(_)
                    | Screen::Search
                    | Screen::Videos
            );
            let source_rows = self.state.rows();
            self.row_artwork = source_rows.iter().map(|row| row.artwork_id).collect();
            let rows =
                source_rows
                    .into_iter()
                    .map(|row| LibraryRow {
                        setting: self.state.is_settings_list(),
                        toggle: self.state.is_settings_list()
                            && self.state.setting_toggle(&row.target).is_some(),
                        checked: self.state.setting_selected(&row.target)
                            || self.state.setting_toggle(&row.target).unwrap_or(false),
                        value: self.text(&self.state.setting_value(&row.target)),
                        icon: self.podcast_row_icon(&row.target).unwrap_or({
                            match (&row.target, screen) {
                        (Target::PlayVideo(_), _) => 9,
                        (Target::GeniusPreview, _) => 1,
                        (Target::Screen(Screen::NowPlaying), Screen::Music) => 8,
                        (Target::Screen(Screen::Groups(Group::Playlists)), Screen::Music) => 2,
                        (Target::Screen(Screen::Groups(Group::Artists)), Screen::Music) => 3,
                        (Target::Screen(Screen::Groups(Group::Albums)), Screen::Music) => 4,
                        (Target::Screen(Screen::Songs(Filter::All)), Screen::Music) => 5,
                        (Target::Screen(Screen::Groups(Group::Genres)), Screen::Music) => 6,
                        (
                            Target::Screen(Screen::Songs(
                                Filter::TopRated
                                | Filter::SmartPlaylist(
                                    crate::library::statistics::SmartPlaylist::MostPlayed
                                    | crate::library::statistics::SmartPlaylist::RecentlyAdded
                                    | crate::library::statistics::SmartPlaylist::NeverPlayed,
                                ),
                            )),
                            _,
                        ) => 7,
                        (Target::Screen(Screen::Songs(_)), Screen::Groups(Group::Playlists)) => 2,
                        _ => 0,
                    }
                        }),
                        title: if screen.is_statistics()
                            || *screen == Screen::LyricSongs
                            || (matches!(screen, Screen::LyricsSearch(_))
                                && matches!(&row.target, Target::Screen(Screen::LyricSongs)))
                            || matches!(
                                &row.target,
                                Target::PlayVideo(_)
                                    | Target::Podcast(crate::podcast::Action::Play(..))
                                    | Target::Screen(Screen::Podcasts(
                                        crate::app::navigation::PodcastPage::Feed(_)
                                    ))
                            )
                            || (metadata_rows && !matches!(&row.target, Target::Action(_)))
                            || matches!(
                                &row.target,
                                Target::Screen(Screen::Songs(Filter::Playlist { .. }))
                            ) {
                            row.title.into()
                        } else {
                            self.text(&row.title)
                        },
                        subtitle: if matches!(row.target, Target::FindLyrics(_)) {
                            i18n::notice(self.state.language, &row.subtitle).into()
                        } else if matches!(row.target, Target::Action(Action::AutomaticLyrics)) {
                            self.text(&row.subtitle)
                        } else if let Target::Shortcut(action) = row.target {
                            self.text(&self.dispatcher.keymap.labels(action))
                        } else if matches!(screen, Screen::Groups(Group::Playlists)) {
                            i18n::notice(self.state.language, &row.subtitle).into()
                        } else {
                            if screen.is_statistics()
                                || metadata_rows
                                || *screen == Screen::LyricSongs
                                || (matches!(screen, Screen::LyricsSearch(_))
                                    && matches!(row.target, Target::Screen(Screen::LyricSongs)))
                                || matches!(
                                    row.target,
                                    Target::Translation(crate::translation::Command::Translate)
                                ) && self.state.current.is_some()
                                || (*screen == Screen::Music
                                    && matches!(row.target, Target::Screen(Screen::NowPlaying)))
                                || matches!(
                                    screen,
                                    Screen::Lyrics(_) | Screen::Languages | Screen::Fonts
                                )
                            {
                                row.subtitle.into()
                            } else {
                                self.text(&row.subtitle)
                            }
                        },
                        artwork: if let Some(image) = self.podcast_row_artwork(&row.target) {
                            image
                        } else if let Target::PlayVideo(id) = row.target {
                            self.state
                                .videos
                                .get(id)
                                .and_then(|video| self.video_thumbnails.cached(&video.path))
                                .unwrap_or_default()
                        } else {
                            row.artwork_id
                                .and_then(|id| self.thumbnails.cached(id))
                                .unwrap_or_default()
                        },
                        show_artwork: matches!(
                            row.target,
                            Target::PlayVideo(_)
                                | Target::Screen(Screen::Podcasts(
                                    crate::app::navigation::PodcastPage::Feed(_)
                                ))
                        ) || (!matches!(row.target, Target::Action(_))
                            && matches!(
                                screen,
                                Screen::Groups(
                                    crate::app::navigation::Group::Artists
                                        | crate::app::navigation::Group::Albums
                                ) | Screen::Artist(_)
                            )),
                        chevron: matches!(
                            row.target,
                            Target::Screen(_)
                                | Target::RefreshStatistics
                                | Target::Shortcut(_)
                                | Target::GeniusPreview
                                | Target::Action(
                                    Action::ConfigureDeepSeek
                                        | Action::ManageLyrics
                                        | Action::ImportLyrics
                                        | Action::ImportFolder
                                        | Action::ImportVideos
                                        | Action::NewPlaylist
                                        | Action::EditPlaylist
                                        | Action::Search
                                )
                        ) && !(*screen == Screen::Music
                            && matches!(row.target, Target::Screen(Screen::NowPlaying))),
                        font_family: if let Target::Font(font) = row.target {
                            font.family().into()
                        } else {
                            "".into()
                        },
                    })
                    .collect::<Vec<_>>();
            self.library_rows = Rc::new(VecModel::from(rows));
            ui.set_rows(self.library_rows.clone().into());
            if screen.is_statistics() {
                self.sync_statistics_view(ui);
            }
            self.last_query.clone_from(&self.state.query);
            self.rows_dirty = false;
            if screen_changed {
                ui.set_list_scroll_y(0.0);
            }
        }
        let scale = ui.get_display_scale();
        if screen_changed
            || self.last_selected != self.state.navigation.selected()
            || (scale - self.last_scale).abs() > 0.01
        {
            if matches!(
                screen,
                Screen::Music
                    | Screen::Statistics(_)
                    | Screen::StatisticsDetail { .. }
                    | Screen::Videos
                    | Screen::Groups(_)
                    | Screen::Artist(_)
                    | Screen::Songs(_)
                    | Screen::Search
                    | Screen::Settings
                    | Screen::SettingsSection(_)
                    | Screen::Shortcuts
                    | Screen::Lyrics(_)
                    | Screen::LyricsSearch(_)
                    | Screen::LyricSongs
                    | Screen::Languages
                    | Screen::Fonts
                    | Screen::Podcasts(_)
            ) {
                let row_height = ui.get_list_row_height();
                let playlist_list = *screen == Screen::Groups(Group::Playlists);
                let row_index = self.state.navigation.selected().saturating_sub(
                    if matches!(
                        screen,
                        Screen::Songs(Filter::Playlist { .. }) | Screen::Groups(Group::Playlists)
                    ) {
                        2
                    } else {
                        usize::from(playlist_list || self.state.supports_list_shuffle())
                    },
                );
                let row_y = row_index as f32 * row_height;
                let visible_height = ui.get_list_viewport_height() - ui.get_list_header_height();
                let scroll = ui.get_list_scroll_y();
                if row_y + scroll < 0.0 {
                    ui.set_list_scroll_y(-row_y);
                } else if row_y + row_height + scroll > visible_height {
                    ui.set_list_scroll_y(visible_height - row_y - row_height);
                }
            }
            self.last_selected = self.state.navigation.selected();
        }
        self.refresh_thumbnails(ui, screen);
        self.refresh_video_thumbnails(ui, screen);
        if screen_changed && self.last_screen.is_some() {
            ui.set_screen_transition_generation(ui.get_screen_transition_generation() + 1);
        }
        self.last_screen = Some(screen.clone());
        self.last_navigation_depth = navigation_depth;
        let track = self.state.current.and_then(|id| self.state.tracks.get(id));
        let artwork_resolution = (240.0 * scale * ui.window().scale_factor()).ceil() as u32;
        let track_changed = self.last_track != self.state.current;
        if track_changed || artwork_resolution != self.artwork_resolution {
            let artwork = track
                .zip(self.state.current)
                .map(|(track, id)| {
                    let preview = self.thumbnails.cached(id).or_else(|| {
                        self.thumbnails
                            .cached_images()
                            .find_map(|(cached_id, image)| {
                                self.state
                                    .tracks
                                    .get(cached_id)
                                    .filter(|cached| {
                                        image.size().width > 0 && cached.artwork == track.artwork
                                    })
                                    .map(|_| image.clone())
                            })
                    });
                    self.artwork.seed_preview(id, preview);
                    self.artwork.request(id, &track.artwork, artwork_resolution)
                })
                .unwrap_or_else(|| {
                    self.artwork.cancel();
                    slint::Image::default()
                });
            // Keep the existing cover visible while a sharper version is prepared on resize.
            if track_changed || artwork.size().width > 0 {
                ui.set_artwork(artwork);
            }
            self.artwork_resolution = artwork_resolution;
        }
        if self.last_track != self.state.current || self.lyrics_dirty {
            ui.set_track_title(track.map_or("", |t| t.title.as_str()).into());
            ui.set_track_artist(track.map_or("", |t| t.artist.as_str()).into());
            ui.set_track_album(track.map_or("", |t| t.album.as_str()).into());
            ui.set_lyric_lines(ModelRc::new(VecModel::from(
                track.map_or_else(Vec::new, |t| {
                    t.lyrics.lines().into_iter().map(Into::into).collect()
                }),
            )));
            ui.set_timed_lyrics(track.is_some_and(|t| !t.lyrics.timed.is_empty()));
            let translated = track.and_then(|track| {
                self.lyrics
                    .cached_translation(track, self.settings.translation_target)
            });
            ui.set_translated_lines(ModelRc::new(VecModel::from(translated.map_or_else(
                Vec::new,
                |translation| {
                    translation
                        .lines
                        .iter()
                        .map(|line| line.as_str().into())
                        .collect()
                },
            ))));
            let romaji = track.and_then(|track| {
                self.state
                    .romaji
                    .get(&track.path)
                    .filter(|romaji| romaji.matches(&track.lyrics))
            });
            ui.set_romaji_lines(ModelRc::new(VecModel::from(romaji.map_or_else(
                Vec::new,
                |romaji| {
                    romaji
                        .lines
                        .iter()
                        .map(|line| line.as_str().into())
                        .collect()
                },
            ))));
            ui.set_lyric_scroll_y(0.0);
            self.last_lyric = None;
            self.last_track = self.state.current;
            self.lyrics_dirty = false;
        }
        let active = track.and_then(|t| t.lyrics.active(self.state.position));
        ui.set_active_lyric(active.map_or(-1, |index| index as i32));
        if self.state.lyrics_visible
            && (active != self.last_lyric || (scale - self.last_scale).abs() > 0.01)
        {
            if active.is_none() {
                ui.set_lyric_scroll_y(0.0);
            }
            // Slint scrolls to the measured row position, including wrapped plain/LRC lines.
            self.last_lyric = active;
        }
        self.last_scale = scale;
        self.sync_podcasts(ui);
    }

    pub(super) fn refresh_thumbnails(&mut self, ui: &AppWindow, screen: &Screen) {
        if self.thumbnails.poll() {
            let artist_images: Vec<_> = if screen.is_statistics() {
                self.state
                    .statistics_entries()
                    .iter()
                    .map(|e| {
                        self.artist_photo_for_target(&e.row.target)
                            .map(|p| p.image.clone())
                    })
                    .collect()
            } else {
                Vec::new()
            };
            for (index, id) in self.row_artwork.iter().enumerate() {
                if let Some(id) = id
                    && let Some(mut row) = self.library_rows.row_data(index)
                {
                    // Release evicted large images even when a synthetic track has no preview.
                    let image = self.thumbnails.cached(*id).unwrap_or_default();
                    if row.artwork != image {
                        row.artwork = image;
                        self.library_rows.set_row_data(index, row);
                    }
                }
                if let Some(id) = id
                    && let Some(mut row) = self.statistics_items.row_data(index)
                {
                    let image = artist_images
                        .get(index)
                        .and_then(|image| image.clone())
                        .or_else(|| self.thumbnails.cached(*id))
                        .unwrap_or_default();
                    if row.artwork != image {
                        row.artwork = image;
                        self.statistics_items.set_row_data(index, row);
                    }
                }
            }
            if screen.is_statistics() {
                let mut summary = ui.get_statistics_summary();
                summary.detail_artwork = self
                    .current_artist_photo()
                    .map(|p| p.image.clone())
                    .or_else(|| {
                        self.state
                            .statistics_detail_info()
                            .artwork_id
                            .and_then(|id| self.thumbnails.cached(id))
                    })
                    .unwrap_or_default();
                ui.set_statistics_summary(summary);
            }
            if let Some(path) = &self.details_path
                && let Some(id) = self.state.tracks.iter().position(|t| &t.path == path)
            {
                ui.set_details_artwork(self.thumbnails.cached(id).unwrap_or_default());
            }
        }
        let resolution = ((ui.global::<ListMetrics>().get_artwork_row_height()
            - 2.0 * ui.global::<ListMetrics>().get_artwork_padding())
            * ui.get_display_scale()
            * ui.window().scale_factor())
        .ceil() as u32;
        let ids = if matches!(
            screen,
            Screen::Groups(Group::Albums | Group::Artists) | Screen::Artist(_)
        ) || screen.is_statistics()
        {
            let height = ui.get_list_row_height().max(1.0);
            let top = (-ui.get_list_scroll_y()).max(0.0);
            let prefix = usize::from(self.state.supports_list_shuffle());
            let start = (top / height).floor() as usize + prefix;
            let end = ((top + ui.get_list_viewport_height() - ui.get_list_header_height()) / height)
                .ceil() as usize
                + prefix;
            self.row_artwork
                .iter()
                .take(end.saturating_add(1))
                .skip(start.saturating_sub(1))
                .filter_map(|id| *id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut ids = ids;
        let mut resolution = resolution;
        if screen.is_statistics()
            && let Some(id) = self.state.statistics_detail_info().artwork_id
        {
            ids.push(id);
            resolution = resolution
                .max((240.0 * ui.get_display_scale() * ui.window().scale_factor()).ceil() as u32);
        }
        if ui.get_track_details_open()
            && let Some(path) = &self.details_path
            && let Some(id) = self.state.tracks.iter().position(|t| &t.path == path)
        {
            ids.push(id);
            resolution = resolution
                .max((96.0 * ui.get_display_scale() * ui.window().scale_factor()).ceil() as u32);
        }
        self.thumbnails
            .set_visible(&self.state.tracks, &ids, resolution);
        self.request_visible_artist_photos(ui, screen);
    }

    fn refresh_video_thumbnails(&mut self, ui: &AppWindow, screen: &Screen) {
        let changed = self.video_thumbnails.poll();
        if *screen != Screen::Videos {
            return;
        }
        if changed {
            // Row zero is Import; the remaining rows retain their video IDs.
            for (id, video) in self.state.videos.iter().enumerate() {
                if let Some(mut row) = self.library_rows.row_data(id + 1) {
                    row.artwork = self
                        .video_thumbnails
                        .cached(&video.path)
                        .unwrap_or_default();
                    self.library_rows.set_row_data(id + 1, row);
                }
            }
        }
        // Request only visible rows plus one on either side, without rebuilding
        // title/subtitle strings. One pending receiver bounds extraction.
        let height = ui.get_list_row_height().max(1.0);
        let start = ((-ui.get_list_scroll_y()).max(0.0) / height).floor() as usize;
        let end = start + (ui.get_list_viewport_height() / height).ceil() as usize + 1;
        for video in self
            .state
            .videos
            .iter()
            .take(end)
            .skip(start.saturating_sub(2))
        {
            if !self.video_thumbnails.request(video) {
                break;
            }
        }
    }

    pub(super) fn sync_editor(&self, ui: &AppWindow) {
        ui.set_shortcut_editor_open(self.shortcut_edit.is_some());
        ui.set_shortcut_recording(self.shortcut_capture.is_some());
        if let Some(action) = self.shortcut_edit {
            ui.set_shortcut_title(self.text(action.title()));
            ui.set_shortcut_keys(self.text(&self.dispatcher.keymap.labels(action)));
        }
    }
}

fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
