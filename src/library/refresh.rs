//! Explicit, finite library reconciliation. No filesystem watchers are retained.
use super::{Track, database::Database, paths, scanner};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub struct Snapshot {
    pub original: Vec<Track>,
    pub tracks: Vec<Track>,
    pub missing: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Run once on a background worker. Metadata stamps reuse unchanged tracks;
/// inaccessible roots and incomplete traversals never imply mass deletion.
pub fn run(database: &Path, roots: &[PathBuf]) -> Result<Snapshot, String> {
    let database = Database::open(database)?;
    let mut original = database.load()?;
    for track in &mut original {
        database.apply_lyrics(track)?;
    }
    let mut roots: Vec<_> = if roots.is_empty() {
        original
            .iter()
            .filter_map(|track| track.path.parent().map(Path::to_path_buf))
            .collect()
    } else {
        roots.iter().map(|root| paths::normalize(root)).collect()
    };
    roots.sort_by_key(|root| root.components().count());
    let mut minimal = Vec::<PathBuf>::new();
    for root in roots {
        if !minimal.iter().any(|parent| root.starts_with(parent)) {
            minimal.push(root);
        }
    }
    let known: Vec<_> = original.iter().map(|track| track.path.clone()).collect();
    let mut candidates = BTreeSet::new();
    let mut missing = BTreeSet::new();
    let mut warnings = Vec::new();
    for root in &minimal {
        match scanner::reconcile_root(root, &known) {
            Ok(snapshot) => {
                candidates.extend(snapshot.paths);
                missing.extend(snapshot.missing);
            }
            Err(error) => warnings.push(format!("{}: {error}", root.display())),
        }
    }
    // Legacy caches may contain files outside the saved import roots. Refresh
    // those files individually without discovering unrelated sibling folders.
    candidates.extend(
        known
            .iter()
            .filter(|path| !minimal.iter().any(|root| path.starts_with(root)))
            .cloned(),
    );
    let mut tracks = Vec::new();
    for path in candidates {
        match scanner::refresh_track(&database, &path) {
            Ok(track) => tracks.push(track),
            Err(error) => warnings.push(format!("{}: {error}", path.display())),
        }
    }
    let mut confirmed_missing = Vec::new();
    for path in missing {
        // A drive may have disconnected while other files were being read, or
        // the user may have put this file back. Verify again before eviction.
        if !minimal
            .iter()
            .any(|root| path.starts_with(root) && root.is_dir())
        {
            continue;
        }
        let absent = match path.metadata() {
            Ok(metadata) => !metadata.is_file(),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
        if absent {
            database.evict_track(&path)?;
            confirmed_missing.push(path);
        }
    }
    Ok(Snapshot {
        original,
        tracks,
        missing: confirmed_missing,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn copy_audio(path: &Path) {
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.wav"),
            path,
        )
        .unwrap();
    }

    #[test]
    fn explicit_refresh_discovers_additions_updates_sidecars_and_removes_missing_rows() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("music");
        fs::create_dir(&root).unwrap();
        let first = root.join("first.wav");
        let second = root.join("second.wav");
        copy_audio(&first);
        let database = directory.path().join("library.sqlite3");
        scanner::scan_folders(&database, std::slice::from_ref(&root)).unwrap();
        copy_audio(&second);
        fs::write(first.with_extension("lrc"), "[00:00]Updated sidecar").unwrap();
        let result = run(&database, std::slice::from_ref(&root)).unwrap();
        assert_eq!(result.original.len(), 1);
        assert_eq!(result.tracks.len(), 2);
        assert!(result.warnings.is_empty());
        assert!(
            result
                .tracks
                .iter()
                .find(|track| track.path == first)
                .unwrap()
                .lyrics
                .lines()[0]
                .contains("Updated sidecar")
        );
        fs::remove_file(&first).unwrap();
        let result = run(&database, std::slice::from_ref(&root)).unwrap();
        assert_eq!(result.missing, [first]);
        assert_eq!(Database::open(&database).unwrap().load().unwrap().len(), 1);
        assert_eq!(result.tracks[0].path, second);
    }

    #[test]
    fn unavailable_roots_preserve_cached_songs_and_restoration_is_discovered() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("music");
        let disconnected = directory.path().join("disconnected");
        fs::create_dir(&root).unwrap();
        let path = root.join("song.wav");
        copy_audio(&path);
        let database = directory.path().join("library.sqlite3");
        scanner::scan_folders(&database, std::slice::from_ref(&root)).unwrap();
        fs::rename(&root, &disconnected).unwrap();
        let snapshot = run(&database, std::slice::from_ref(&root)).unwrap();
        assert!(snapshot.missing.is_empty());
        assert_eq!(Database::open(&database).unwrap().load().unwrap().len(), 1);
        fs::rename(&disconnected, &root).unwrap();
        assert_eq!(run(&database, &[root]).unwrap().tracks[0].path, path);
    }

    #[test]
    fn empty_imported_roots_and_legacy_cache_parents_can_discover_new_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("music");
        fs::create_dir(&root).unwrap();
        let database = directory.path().join("library.sqlite3");
        assert!(
            run(&database, std::slice::from_ref(&root))
                .unwrap()
                .tracks
                .is_empty()
        );
        copy_audio(&root.join("first.wav"));
        assert_eq!(
            run(&database, &[root.clone(), root.clone()])
                .unwrap()
                .tracks
                .len(),
            1
        );
        copy_audio(&root.join("second.wav"));
        assert_eq!(run(&database, &[]).unwrap().tracks.len(), 2);
    }
}
