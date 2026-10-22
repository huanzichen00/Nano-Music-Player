//! Small desktop projection of worker-owned listening statistics.
use super::{
    i18n::{Language, translate},
    navigation::{Filter, Screen, StatisticsPage},
    state::{Row, State, Target},
};
use crate::library::statistics::{StatisticsRange, StatisticsSnapshot};

#[derive(Default)]
pub struct StatisticsState {
    pub range: StatisticsRange,
    pub snapshot: Option<StatisticsSnapshot>,
    pub loading: bool,
    pub failed: bool,
}

fn plays(language: Language, count: u64) -> String {
    translate(language, "{0} plays").replace("{0}", &count.to_string())
}

impl State {
    pub(crate) fn clamp_statistics_selection(&mut self) {
        if matches!(self.navigation.current().screen, Screen::Statistics(_)) {
            let last = self.selection_count().saturating_sub(1);
            if self.navigation.selected() > last {
                self.navigation.select(last);
            }
        }
    }

    pub fn statistics_note(&self) -> String {
        let tr = |key| translate(self.language, key);
        let range = if self.navigation.current().screen == Screen::Statistics(StatisticsPage::Daily)
        {
            tr("Last 30 Days")
        } else {
            tr(self.statistics.range.label())
        };
        let status = if self.statistics.loading {
            tr("Loading…")
        } else if self.statistics.failed {
            tr("Unable to load. Refresh to retry.")
        } else {
            tr("Local calendar days")
        };
        // Keep the retention warning visible even when the current library is
        // empty: historical plays may outlive their source files.
        format!(
            "{range} · {status}\n{}",
            tr("History: latest 1,000 plays.\nDate totals may be incomplete.")
        )
    }

    pub(super) fn statistics_rows(&self, page: StatisticsPage) -> Vec<Row> {
        let tr = |key| translate(self.language, key);
        let row = |title: String, subtitle: String, target| Row {
            title,
            subtitle,
            artwork_id: None,
            target,
        };
        let link = |page: StatisticsPage, subtitle: String| {
            row(
                tr(page.title()),
                subtitle,
                Target::Screen(Screen::Statistics(page)),
            )
        };
        let empty = || {
            vec![
                row(
                    tr(if self.statistics.loading {
                        "Loading…"
                    } else if self.statistics.failed {
                        "Unable to load statistics"
                    } else {
                        "No plays yet"
                    }),
                    String::new(),
                    Target::None,
                ),
                row(tr("Refresh"), String::new(), Target::RefreshStatistics),
            ]
        };
        if page == StatisticsPage::Home {
            return vec![
                link(
                    StatisticsPage::Overview,
                    self.statistics
                        .snapshot
                        .as_ref()
                        .map_or_else(String::new, |s| plays(self.language, s.total_plays)),
                ),
                link(StatisticsPage::Artists, String::new()),
                link(StatisticsPage::Albums, String::new()),
                link(StatisticsPage::Tracks, String::new()),
                link(StatisticsPage::Recent, String::new()),
                link(StatisticsPage::Daily, tr("Last 30 Days")),
                link(StatisticsPage::Range, tr(self.statistics.range.label())),
                row(tr("Refresh"), String::new(), Target::RefreshStatistics),
            ];
        }
        if page == StatisticsPage::Range {
            return StatisticsRange::ALL
                .into_iter()
                .map(|range| {
                    row(
                        tr(range.label()),
                        if range == self.statistics.range {
                            tr("Selected")
                        } else {
                            String::new()
                        },
                        Target::StatisticsRange(range),
                    )
                })
                .collect();
        }
        let Some(snapshot) = &self.statistics.snapshot else {
            return empty();
        };
        let path_target = |path: &std::path::Path| {
            self.tracks
                .iter()
                .enumerate()
                .find(|(_, track)| track.path == path && self.track_available(track))
                .map_or(Target::None, |(id, _)| Target::Play(id))
        };
        let group_target = |name: String, paths: Vec<std::path::PathBuf>, album_order: bool| {
            Target::Screen(Screen::Songs(Filter::Statistics {
                name,
                album_order,
                paths: std::sync::Arc::new(paths.into_iter().collect()),
            }))
        };
        let rows: Vec<_> = match page {
            StatisticsPage::Overview => vec![
                row(
                    tr("Total Plays"),
                    plays(self.language, snapshot.total_plays),
                    Target::None,
                ),
                link(
                    StatisticsPage::Artists,
                    snapshot.top_artists.first().map_or_else(
                        || tr("No plays yet"),
                        |a| format!("{} · {}", plays(self.language, a.plays), a.name),
                    ),
                ),
                link(
                    StatisticsPage::Albums,
                    snapshot.top_albums.first().map_or_else(
                        || tr("No plays yet"),
                        |a| format!("{} · {}", plays(self.language, a.plays), a.name),
                    ),
                ),
                link(
                    StatisticsPage::Tracks,
                    snapshot.top_tracks.first().map_or_else(
                        || tr("No plays yet"),
                        |a| format!("{} · {}", plays(self.language, a.plays), a.track.title),
                    ),
                ),
                row(
                    tr("Totals include removed songs"),
                    tr("Rankings use available songs"),
                    Target::None,
                ),
            ],
            StatisticsPage::Artists => snapshot
                .top_artists
                .iter()
                .map(|a| {
                    row(
                        a.name.clone(),
                        plays(self.language, a.plays),
                        group_target(a.name.clone(), a.paths.clone(), false),
                    )
                })
                .collect(),
            StatisticsPage::Albums => snapshot
                .top_albums
                .iter()
                .map(|a| {
                    row(
                        a.name.clone(),
                        format!("{} · {}", plays(self.language, a.plays), a.artist),
                        group_target(a.name.clone(), a.paths.clone(), true),
                    )
                })
                .collect(),
            StatisticsPage::Tracks => snapshot
                .top_tracks
                .iter()
                .map(|a| {
                    row(
                        a.track.title.clone(),
                        format!("{} · {}", plays(self.language, a.plays), a.track.artist),
                        path_target(&a.track.path),
                    )
                })
                .collect(),
            StatisticsPage::Recent => snapshot
                .recently_played
                .iter()
                .map(|a| {
                    let date = chrono::DateTime::from_timestamp(a.played_at, 0)
                        .map(|d| {
                            d.with_timezone(&chrono::Local)
                                .format("%m-%d %H:%M")
                                .to_string()
                        })
                        .unwrap_or_default();
                    row(
                        a.track.title.clone(),
                        format!(
                            "{} · {date}{}",
                            a.track.artist,
                            if a.available {
                                String::new()
                            } else {
                                format!(" · {}", tr("Unavailable"))
                            }
                        ),
                        if a.available {
                            group_target(a.track.title.clone(), vec![a.track.path.clone()], false)
                        } else {
                            Target::None
                        },
                    )
                })
                .collect(),
            StatisticsPage::Daily => snapshot
                .daily_plays
                .iter()
                .rev()
                .map(|a| {
                    row(
                        a.date.format("%Y-%m-%d").to_string(),
                        plays(self.language, a.plays),
                        Target::None,
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        if rows.is_empty() { empty() } else { rows }
    }
}
