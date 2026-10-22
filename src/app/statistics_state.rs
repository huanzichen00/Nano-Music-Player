//! Desktop statistics rows and detail navigation, projected from worker snapshots.
use super::{
    i18n::{Language, translate},
    navigation::{Filter, Screen, StatisticsDetailPage, StatisticsGroup, StatisticsPage},
    state::{Row, State, Target},
    statistics_projection::{bar_ratio, grouped_number, relative_played_at},
};
use crate::library::statistics::{
    StatisticsAlbum, StatisticsRange, StatisticsSnapshot, statistics_identity,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub struct StatisticsState {
    pub range: StatisticsRange,
    pub snapshot: Option<StatisticsSnapshot>,
    pub loading: bool,
    pub failed: bool,
}

pub struct StatisticsEntry {
    pub row: Row,
    pub value: String,
    pub rank: i32,
    pub ratio: f32,
    pub icon: i32,
}

#[derive(Default)]
pub struct StatisticsDetailInfo {
    pub title: String,
    pub subtitle: String,
    pub artwork_id: Option<usize>,
}

fn plays(language: Language, count: u64) -> String {
    translate(language, "{0} plays").replace("{0}", &grouped_number(count))
}

fn group(name: String, artist: Option<String>, paths: Vec<PathBuf>) -> StatisticsGroup {
    StatisticsGroup {
        name,
        artist,
        paths: Arc::new(paths.into_iter().collect()),
    }
}

fn entry(
    title: String,
    subtitle: String,
    artwork_id: Option<usize>,
    target: Target,
) -> StatisticsEntry {
    StatisticsEntry {
        row: Row {
            title,
            subtitle,
            artwork_id,
            target,
        },
        value: String::new(),
        rank: 0,
        ratio: 0.0,
        icon: 0,
    }
}

fn detail_target(group: StatisticsGroup, page: StatisticsDetailPage) -> Target {
    Target::Screen(Screen::StatisticsDetail { group, page })
}

impl State {
    pub(crate) fn clamp_statistics_selection(&mut self) {
        if self.navigation.current().screen.is_statistics() {
            let last = self.selection_count().saturating_sub(1);
            if self.navigation.selected() > last {
                self.navigation.select(last);
            }
        }
    }

    pub fn statistics_note(&self) -> String {
        let tr = |key| translate(self.language, key);
        let prefix = if self.statistics.failed {
            tr("Load failed. Tap to retry.")
        } else if self.statistics.loading {
            tr("Loading…")
        } else if self.navigation.current().screen == Screen::Statistics(StatisticsPage::Daily) {
            tr("Last 30 Days")
        } else {
            tr(self.statistics.range.label())
        };
        format!(
            "{prefix} · {}",
            tr("Latest 1,000 plays; date totals may be partial.")
        )
    }

    pub fn statistics_counts(&self) -> (u64, u64, u64, u64) {
        self.statistics.snapshot.as_ref().map_or((0, 0, 0, 0), |s| {
            (
                s.total_plays,
                s.top_artists.len() as u64,
                s.top_albums.len() as u64,
                s.top_tracks.len() as u64,
            )
        })
    }

    pub fn statistics_artwork(&self, paths: &BTreeSet<PathBuf>) -> Option<usize> {
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                paths.contains(&t.path) && self.track_available(t) && !t.artwork.is_empty()
            })
            .min_by(|(_, a), (_, b)| a.path.cmp(&b.path))
            .map(|(id, _)| id)
    }

    fn statistics_track_id(&self, path: &Path) -> Option<usize> {
        self.tracks
            .iter()
            .enumerate()
            .find(|(_, t)| t.path == path && self.track_available(t))
            .map(|(id, _)| id)
    }

    fn statistics_group_count(&self, group: &StatisticsGroup) -> u64 {
        self.statistics.snapshot.as_ref().map_or(0, |s| {
            s.top_tracks
                .iter()
                .filter(|t| {
                    group.paths.contains(&t.track.path)
                        && self.statistics_track_id(&t.track.path).is_some()
                })
                .fold(0_u64, |count, t| count.saturating_add(t.plays))
        })
    }

    fn statistics_group_size(&self, group: &StatisticsGroup) -> usize {
        self.tracks
            .iter()
            .filter(|t| group.paths.contains(&t.path) && self.track_available(t))
            .count()
    }

    pub fn statistics_detail_info(&self) -> StatisticsDetailInfo {
        let Screen::StatisticsDetail { group, .. } = &self.navigation.current().screen else {
            return StatisticsDetailInfo::default();
        };
        let count = self.statistics_group_count(group);
        let tracks = self.statistics_group_size(group);
        let count_and_tracks = translate(self.language, "{0} plays · {1} tracks")
            .replace("{0}", &grouped_number(count))
            .replace("{1}", &grouped_number(tracks as u64));
        StatisticsDetailInfo {
            title: group.name.clone(),
            subtitle: group
                .artist
                .as_ref()
                .map_or(count_and_tracks.clone(), |artist| {
                    format!("{artist}\n{count_and_tracks}")
                }),
            artwork_id: self.statistics_artwork(&group.paths),
        }
    }

    fn statistics_albums(&self, group: &StatisticsGroup) -> Vec<StatisticsAlbum> {
        let counts: HashMap<_, _> = self
            .statistics
            .snapshot
            .as_ref()
            .map(|s| {
                s.top_tracks
                    .iter()
                    .map(|t| (&t.track.path, t.plays))
                    .collect()
            })
            .unwrap_or_default();
        let mut albums = BTreeMap::<(String, String), StatisticsAlbum>::new();
        for t in self
            .tracks
            .iter()
            .filter(|t| group.paths.contains(&t.path) && self.track_available(t))
        {
            let key = (
                statistics_identity(&t.album),
                statistics_identity(&t.artist),
            );
            let album = albums.entry(key).or_insert_with(|| StatisticsAlbum {
                name: t.album.clone(),
                artist: t.artist.clone(),
                plays: 0,
                paths: Vec::new(),
            });
            album.name = album.name.clone().min(t.album.clone());
            album.artist = album.artist.clone().min(t.artist.clone());
            album.plays = album
                .plays
                .saturating_add(counts.get(&t.path).copied().unwrap_or(0));
            album.paths.push(t.path.clone());
        }
        let mut albums: Vec<_> = albums.into_iter().collect();
        albums.sort_by(|(ka, a), (kb, b)| b.plays.cmp(&a.plays).then_with(|| ka.cmp(kb)));
        albums.into_iter().map(|(_, a)| a).collect()
    }

    fn statistics_album_entries(&self, albums: &[StatisticsAlbum]) -> Vec<StatisticsEntry> {
        let max = albums.first().map_or(0, |a| a.plays);
        albums
            .iter()
            .enumerate()
            .map(|(index, a)| {
                let g = group(a.name.clone(), Some(a.artist.clone()), a.paths.clone());
                let art = self.statistics_artwork(&g.paths);
                let mut e = entry(
                    a.name.clone(),
                    format!("{} · {}", plays(self.language, a.plays), a.artist),
                    art,
                    detail_target(g, StatisticsDetailPage::Summary),
                );
                e.rank = index as i32 + 1;
                e.value = e.row.subtitle.clone();
                e.ratio = bar_ratio(a.plays, max);
                e
            })
            .collect()
    }

    fn statistics_track_entries(&self, group: Option<&StatisticsGroup>) -> Vec<StatisticsEntry> {
        let Some(s) = &self.statistics.snapshot else {
            return Vec::new();
        };
        s.top_tracks
            .iter()
            .filter(|t| group.is_none_or(|g| g.paths.contains(&t.track.path)))
            .enumerate()
            .map(|(index, t)| {
                let id = self.statistics_track_id(&t.track.path);
                let mut e = entry(
                    t.track.title.clone(),
                    t.track.artist.clone(),
                    id,
                    id.map_or(Target::None, Target::Play),
                );
                e.rank = index as i32 + 1;
                e.value = plays(self.language, t.plays);
                e
            })
            .collect()
    }

    fn statistics_artist_entries(&self) -> Vec<StatisticsEntry> {
        let Some(s) = &self.statistics.snapshot else {
            return Vec::new();
        };
        let max = s.top_artists.first().map_or(0, |a| a.plays);
        s.top_artists
            .iter()
            .enumerate()
            .map(|(index, a)| {
                let g = group(a.name.clone(), None, a.paths.clone());
                let art = self.statistics_artwork(&g.paths);
                let mut e = entry(
                    a.name.clone(),
                    plays(self.language, a.plays),
                    art,
                    detail_target(g, StatisticsDetailPage::Summary),
                );
                e.rank = index as i32 + 1;
                e.value = e.row.subtitle.clone();
                e.ratio = bar_ratio(a.plays, max);
                e
            })
            .collect()
    }

    fn statistics_empty_entries(&self) -> Vec<StatisticsEntry> {
        let tr = |key| translate(self.language, key);
        vec![
            entry(
                tr(if self.statistics.loading {
                    "Loading…"
                } else if self.statistics.failed {
                    "Unable to load statistics"
                } else {
                    "No plays yet"
                }),
                String::new(),
                None,
                Target::None,
            ),
            entry(
                tr("Refresh"),
                String::new(),
                None,
                Target::RefreshStatistics,
            ),
        ]
    }

    pub fn statistics_entries(&self) -> Vec<StatisticsEntry> {
        let tr = |key| translate(self.language, key);
        let mut rows = match &self.navigation.current().screen {
            Screen::Statistics(StatisticsPage::Home) => [
                (StatisticsPage::Overview, "Summary and charts", 1),
                (StatisticsPage::Artists, "Most played artists", 2),
                (StatisticsPage::Albums, "Most played albums", 3),
                (StatisticsPage::Tracks, "Most played songs", 4),
                (StatisticsPage::Recent, "Latest retained plays", 5),
                (StatisticsPage::Daily, "Last 30 Days", 6),
                (StatisticsPage::Range, self.statistics.range.label(), 7),
            ]
            .into_iter()
            .map(|(p, subtitle, icon)| {
                let mut e = entry(
                    tr(p.title()),
                    tr(subtitle),
                    None,
                    Target::Screen(Screen::Statistics(p)),
                );
                e.icon = icon;
                e
            })
            .chain(std::iter::once({
                let mut e = entry(
                    tr("Refresh"),
                    String::new(),
                    None,
                    Target::RefreshStatistics,
                );
                e.icon = 8;
                e
            }))
            .collect(),
            Screen::Statistics(StatisticsPage::Range) => StatisticsRange::ALL
                .into_iter()
                .map(|range| {
                    entry(
                        tr(range.label()),
                        if range == self.statistics.range {
                            tr("Selected")
                        } else {
                            String::new()
                        },
                        None,
                        Target::StatisticsRange(range),
                    )
                })
                .collect(),
            Screen::Statistics(StatisticsPage::Overview) => self
                .statistics_artist_entries()
                .into_iter()
                .take(3)
                .collect(),
            Screen::Statistics(StatisticsPage::Artists) => self.statistics_artist_entries(),
            Screen::Statistics(StatisticsPage::Albums) => self
                .statistics
                .snapshot
                .as_ref()
                .map_or_else(Vec::new, |s| self.statistics_album_entries(&s.top_albums)),
            Screen::Statistics(StatisticsPage::Tracks) => self.statistics_track_entries(None),
            Screen::Statistics(StatisticsPage::Recent) => self
                .statistics
                .snapshot
                .as_ref()
                .map_or_else(Vec::new, |s| {
                    s.recently_played
                        .iter()
                        .map(|r| {
                            let id = self.statistics_track_id(&r.track.path);
                            let target = if r.available && id.is_some() {
                                Target::Screen(Screen::Songs(Filter::Statistics {
                                    name: r.track.title.clone(),
                                    album_order: false,
                                    paths: Arc::new(BTreeSet::from([r.track.path.clone()])),
                                }))
                            } else {
                                Target::None
                            };
                            let mut e = entry(
                                r.track.title.clone(),
                                if r.available {
                                    r.track.artist.clone()
                                } else {
                                    tr("Unavailable")
                                },
                                id,
                                target,
                            );
                            e.value = relative_played_at(
                                r.played_at,
                                chrono::Local::now(),
                                self.language,
                            );
                            e
                        })
                        .collect()
                }),
            Screen::Statistics(StatisticsPage::Daily) => self
                .statistics
                .snapshot
                .as_ref()
                .map_or_else(Vec::new, |s| {
                    let max = s.daily_plays.iter().map(|d| d.plays).max().unwrap_or(0);
                    s.daily_plays
                        .iter()
                        .rev()
                        .map(|day| {
                            let mut e = entry(
                                day.date.format("%Y-%m-%d").to_string(),
                                String::new(),
                                None,
                                Target::None,
                            );
                            e.value = grouped_number(day.plays);
                            e.ratio = bar_ratio(day.plays, max);
                            e
                        })
                        .collect()
                }),
            Screen::StatisticsDetail {
                group,
                page: StatisticsDetailPage::Albums,
            } => self.statistics_album_entries(&self.statistics_albums(group)),
            Screen::StatisticsDetail {
                group,
                page: StatisticsDetailPage::Tracks,
            } => self.statistics_track_entries(Some(group)),
            Screen::StatisticsDetail {
                group,
                page: StatisticsDetailPage::Summary,
            } => {
                let size = self.statistics_group_size(group);
                let play = if size > 0 {
                    Target::PlayStatisticsGroup(group.clone())
                } else {
                    Target::None
                };
                let mut result = vec![{
                    let mut e = entry(
                        tr(if group.artist.is_some() {
                            "Play Album"
                        } else {
                            "Play Artist"
                        }),
                        String::new(),
                        None,
                        play,
                    );
                    e.icon = 10;
                    e
                }];
                if group.artist.is_none() {
                    let mut e = entry(
                        tr("Albums"),
                        String::new(),
                        None,
                        detail_target(group.clone(), StatisticsDetailPage::Albums),
                    );
                    e.icon = 11;
                    e.value = grouped_number(self.statistics_albums(group).len() as u64);
                    result.push(e);
                }
                let tracks = self.statistics_track_entries(Some(group));
                let mut top = entry(
                    tr("Top Tracks"),
                    String::new(),
                    None,
                    detail_target(group.clone(), StatisticsDetailPage::Tracks),
                );
                top.icon = 12;
                top.value = grouped_number(tracks.len() as u64);
                result.push(top);
                let mut all = entry(
                    tr("See All Tracks"),
                    String::new(),
                    None,
                    Target::Screen(Screen::Songs(group.filter())),
                );
                all.icon = 13;
                all.value = grouped_number(size as u64);
                result.push(all);
                if group.artist.is_some() {
                    result.extend(tracks.into_iter().take(5));
                }
                result
            }
            _ => Vec::new(),
        };
        if rows.is_empty() && self.navigation.current().screen.is_statistics() {
            rows = self.statistics_empty_entries();
        }
        rows
    }
}
