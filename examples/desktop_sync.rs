//! Exercise the same verified import used on iPhone against a local Documents directory.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 1 {
        return Err(
            "Usage: desktop-sync <Documents directory containing DesktopSync packages>".into(),
        );
    }
    let documents = std::path::Path::new(&arguments[0]);
    let reports = nano_music_player::library::desktop_sync::apply_pending(documents);
    println!("{}", serde_json::to_string_pretty(&reports)?);
    if reports.is_empty() || reports.iter().any(|report| !report.complete) {
        return Err("No completed desktop sync import; inspect the package report.json".into());
    }
    Ok(())
}
