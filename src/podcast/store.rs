use super::{Episode, Feed, Library, ParsedFeed};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub struct Store {
    db: Connection,
    root: PathBuf,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self, String> {
        let root = directory.join("podcasts");
        std::fs::create_dir_all(&root).map_err(error)?;
        let db = Connection::open(directory.join("podcasts.sqlite3")).map_err(error)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        db.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS feeds(id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE, data TEXT NOT NULL, subscribed INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS feed_aliases(url TEXT PRIMARY KEY, feed_id INTEGER NOT NULL REFERENCES feeds(id));
            CREATE TABLE IF NOT EXISTS episodes(id INTEGER PRIMARY KEY, feed_id INTEGER NOT NULL REFERENCES feeds(id), guid TEXT, data TEXT NOT NULL, position REAL NOT NULL DEFAULT 0, completed INTEGER NOT NULL DEFAULT 0, last_played INTEGER NOT NULL DEFAULT 0, local_file TEXT, actual_duration REAL NOT NULL DEFAULT 0);
            CREATE UNIQUE INDEX IF NOT EXISTS episode_guid ON episodes(feed_id,guid) WHERE guid IS NOT NULL;
            CREATE TABLE IF NOT EXISTS episode_aliases(feed_id INTEGER NOT NULL, kind TEXT NOT NULL, value TEXT NOT NULL, episode_id INTEGER NOT NULL REFERENCES episodes(id), PRIMARY KEY(feed_id,kind,value));
            PRAGMA user_version=1;").map_err(error)?;
        // Incomplete transfers cannot be offered as playable downloads after restart.
        for file in std::fs::read_dir(&root).map_err(error)?.flatten() {
            if file.path().extension().is_some_and(|v| v == "part") {
                let _ = std::fs::remove_file(file.path());
            }
        }
        let store = Self { db, root };
        let library = store.library()?;
        for episode in library.episodes {
            if episode.local_path.as_ref().is_some_and(|p| !p.is_file()) {
                store
                    .db
                    .execute(
                        "UPDATE episodes SET local_file=NULL WHERE id=?",
                        [episode.id],
                    )
                    .map_err(error)?;
            }
            if episode.local_path.is_none() && store.path(episode.id).is_file() {
                // A verified transfer can be renamed immediately before a crash,
                // before its database acknowledgement has been committed.
                store.downloaded(episode.id, 0.0)?;
            }
        }
        Ok(store)
    }

    pub fn merge(
        &mut self,
        mut parsed: ParsedFeed,
        refresh_id: Option<i64>,
    ) -> Result<i64, String> {
        let tx = self.db.transaction().map_err(error)?;
        let mut id = refresh_id;
        if id.is_none() {
            for url in std::iter::once(&parsed.feed.url).chain(parsed.aliases.iter()) {
                id = tx
                    .query_row("SELECT feed_id FROM feed_aliases WHERE url=?", [url], |r| {
                        r.get(0)
                    })
                    .optional()
                    .map_err(error)?;
                if id.is_some() {
                    break;
                }
            }
        }
        let data = serde_json::to_string(&parsed.feed).map_err(error)?;
        let id = if let Some(id) = id {
            tx.execute(
                "UPDATE feeds SET url=?,data=?,subscribed=? WHERE id=?",
                params![parsed.feed.url, data, parsed.feed.subscribed, id],
            )
            .map_err(error)?;
            id
        } else {
            tx.execute(
                "INSERT INTO feeds(url,data,subscribed) VALUES(?,?,1)",
                params![parsed.feed.url, data],
            )
            .map_err(error)?;
            tx.last_insert_rowid()
        };
        for url in std::iter::once(parsed.feed.url).chain(parsed.aliases) {
            let owner: Option<i64> = tx
                .query_row(
                    "SELECT feed_id FROM feed_aliases WHERE url=?",
                    [&url],
                    |r| r.get(0),
                )
                .optional()
                .map_err(error)?;
            if owner.is_some_and(|owner| owner != id) {
                return Err("This RSS URL already belongs to another subscription".into());
            }
            tx.execute(
                "INSERT OR IGNORE INTO feed_aliases VALUES(?,?)",
                params![url, id],
            )
            .map_err(error)?;
        }
        for episode in &mut parsed.episodes {
            episode.feed_id = id;
            let mut existing: Option<i64> = if let Some(guid) = &episode.guid {
                tx.query_row(
                    "SELECT id FROM episodes WHERE feed_id=? AND guid=?",
                    params![id, guid],
                    |r| r.get(0),
                )
                .optional()
                .map_err(error)?
            } else {
                None
            };
            let key = episode
                .media_url
                .clone()
                .unwrap_or_else(|| format!("{}|{:?}", episode.title, episode.published));
            let kind = if episode.media_url.is_some() {
                "url"
            } else {
                "metadata"
            };
            if existing.is_none() {
                let candidate: Option<(i64,Option<String>)> = tx.query_row("SELECT e.id,e.guid FROM episode_aliases a JOIN episodes e ON e.id=a.episode_id WHERE a.feed_id=? AND a.kind=? AND a.value=?", params![id,kind,key], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(error)?;
                if let Some((candidate, old_guid)) = candidate {
                    // A new GUID may upgrade a GUID-less item, but cannot merge two distinct GUIDs.
                    if old_guid.is_none() || episode.guid.is_none() || old_guid == episode.guid {
                        existing = Some(candidate);
                    }
                }
            }
            let data = serde_json::to_string(episode).map_err(error)?;
            let episode_id = if let Some(existing) = existing {
                tx.execute(
                    "UPDATE episodes SET guid=COALESCE(?,guid),data=? WHERE id=?",
                    params![episode.guid, data, existing],
                )
                .map_err(error)?;
                existing
            } else {
                tx.execute(
                    "INSERT INTO episodes(feed_id,guid,data) VALUES(?,?,?)",
                    params![id, episode.guid, data],
                )
                .map_err(error)?;
                tx.last_insert_rowid()
            };
            tx.execute(
                "INSERT OR IGNORE INTO episode_aliases VALUES(?,?,?,?)",
                params![id, kind, key, episode_id],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        Ok(id)
    }

    pub fn library(&self) -> Result<Library, String> {
        let mut feeds = self
            .db
            .prepare("SELECT id,data,subscribed FROM feeds ORDER BY id")
            .map_err(error)?;
        let feeds = feeds
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(error)?
            .map(|r| {
                let (id, data, subscribed) = r.map_err(error)?;
                let mut feed: Feed = serde_json::from_str(&data).map_err(error)?;
                feed.id = id;
                feed.subscribed = subscribed;
                Ok(feed)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut query = self.db.prepare("SELECT id,feed_id,data,position,completed,last_played,local_file,actual_duration FROM episodes ORDER BY last_played DESC,id DESC").map_err(error)?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            })
            .map_err(error)?;
        let episodes = rows
            .map(|row| {
                let (id, feed_id, data, position, completed, last_played, file, duration) =
                    row.map_err(error)?;
                let mut episode: Episode = serde_json::from_str(&data).map_err(error)?;
                episode.id = id;
                episode.feed_id = feed_id;
                episode.position = position;
                episode.completed = completed;
                episode.last_played = last_played;
                episode.local_path = file
                    .filter(|f| *f == format!("{id}.audio"))
                    .map(|f| self.root.join(f));
                if duration > 0.0 {
                    episode.duration = duration;
                }
                Ok(episode)
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Library { feeds, episodes })
    }

    pub fn progress(&self, id: i64, position: f64, completed: Option<bool>) -> Result<(), String> {
        if !position.is_finite() || position < 0.0 {
            return Err("Invalid podcast position".into());
        }
        self.db.execute("UPDATE episodes SET position=?,completed=COALESCE(?,completed),last_played=? WHERE id=?", params![position,completed,chrono::Utc::now().timestamp(),id]).map_err(error)?;
        Ok(())
    }
    pub fn unsubscribe(&self, id: i64) -> Result<(), String> {
        self.db
            .execute("UPDATE feeds SET subscribed=0 WHERE id=?", [id])
            .map_err(error)?;
        Ok(())
    }
    pub fn downloaded(&self, id: i64, duration: f64) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE episodes SET local_file=?,actual_duration=? WHERE id=?",
                params![format!("{id}.audio"), duration, id],
            )
            .map_err(error)?;
        Ok(())
    }
    pub fn delete_download(&self, id: i64) -> Result<(), String> {
        match std::fs::remove_file(self.path(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(e)),
        }
        self.db
            .execute("UPDATE episodes SET local_file=NULL WHERE id=?", [id])
            .map_err(error)?;
        Ok(())
    }
    pub fn path(&self, id: i64) -> PathBuf {
        self.root.join(format!("{id}.audio"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parsed(guid: Option<&str>, media: &str) -> ParsedFeed {
        ParsedFeed {
            feed: Feed {
                url: "https://example.org/feed?token=x".into(),
                title: "Show".into(),
                subscribed: true,
                ..Feed::default()
            },
            episodes: vec![Episode {
                guid: guid.map(str::to_owned),
                title: "Episode".into(),
                media_url: Some(media.into()),
                ..Episode::default()
            }],
            aliases: vec![],
        }
    }
    #[test]
    fn refresh_upgrades_guid_preserves_local_state_and_handles_changed_urls() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let feed = store
            .merge(parsed(None, "https://e.org/1.mp3"), None)
            .unwrap();
        let id = store.library().unwrap().episodes[0].id;
        store.progress(id, 123.5, Some(true)).unwrap();
        store
            .merge(parsed(Some("guid"), "https://e.org/1.mp3"), Some(feed))
            .unwrap();
        store
            .merge(
                parsed(Some("guid"), "https://e.org/changed.mp3"),
                Some(feed),
            )
            .unwrap();
        let lib = store.library().unwrap();
        assert_eq!(lib.episodes.len(), 1);
        assert_eq!(lib.episodes[0].id, id);
        assert_eq!(lib.episodes[0].position, 123.5);
        assert!(lib.episodes[0].completed);
        store
            .merge(
                parsed(Some("different-guid"), "https://e.org/changed.mp3"),
                Some(feed),
            )
            .unwrap();
        assert_eq!(store.library().unwrap().episodes.len(), 2);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.library().unwrap().feeds.len(), 1);
    }
}
