#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(target_os = "ios", feature = "mobile"))]
    {
        nano_music_player::mobile::run()
    }
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    {
        nano_music_player::app::run()
    }
}
