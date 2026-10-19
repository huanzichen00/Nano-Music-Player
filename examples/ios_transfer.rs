//! Run the computer import interface without opening the player or an audio device.
use nano_music_player::mobile::{
    storage::Storage,
    transfer::{Transfer, TransferEvent},
};
use std::{path::PathBuf, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut directory = PathBuf::from("target/ios-transfer-preview");
    let mut duration = 0u64;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--config-dir" => {
                directory = args.next().ok_or("--config-dir needs a directory")?.into();
            }
            "--duration-seconds" => {
                duration = args
                    .next()
                    .ok_or("--duration-seconds needs a number")?
                    .parse()?;
            }
            "--help" | "-h" => {
                println!(
                    "ios-transfer [--config-dir DIRECTORY] [--duration-seconds SECONDS]\n\
                     Starts a temporary computer-import server. Zero seconds means until Ctrl+C."
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown argument: {argument}").into()),
        }
    }
    let storage = Storage::new(directory).map_err(std::io::Error::other)?;
    let mut transfer = Transfer::start(storage).map_err(std::io::Error::other)?;
    println!("Open one of these complete addresses in your computer browser:");
    for url in transfer.urls() {
        println!("{url}");
    }
    let started = std::time::Instant::now();
    while transfer.is_running() && (duration == 0 || started.elapsed().as_secs() < duration) {
        while let Some(event) = transfer.poll_event() {
            match event {
                TransferEvent::LibraryChanged(library) => {
                    println!("Library: {} songs", library.tracks.len());
                    for warning in library.warnings {
                        eprintln!("{warning}");
                    }
                }
                TransferEvent::Notice(message) => println!("{message}"),
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    transfer.stop();
    Ok(())
}
