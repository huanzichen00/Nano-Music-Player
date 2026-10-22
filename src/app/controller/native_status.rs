use super::*;
use crate::platform::macos_media::RemoteCommand;

impl Controller {
    fn active_transport(&self) -> (bool, bool, PlaybackStatus) {
        if self.state.podcasts.active.is_some() {
            (true, self.state.podcasts.paused, self.state.podcasts.status)
        } else if self.state.video_active {
            (
                true,
                self.state.video_paused,
                match self.state.video_status {
                    crate::video::Status::Playing => PlaybackStatus::Playing,
                    crate::video::Status::Loading => PlaybackStatus::Loading,
                    crate::video::Status::Paused => PlaybackStatus::Paused,
                    crate::video::Status::Error => PlaybackStatus::Error,
                    _ => PlaybackStatus::Stopped,
                },
            )
        } else {
            (
                self.state.current.is_some(),
                self.state.paused,
                self.state.status,
            )
        }
    }

    pub(super) fn pause_for_output_change(&mut self, ui: &AppWindow) {
        let (has_item, paused, status) = self.active_transport();
        crate::shared::diagnostics::record(
            "desktop_output_changed",
            serde_json::json!({"paused": paused, "status": format!("{status:?}")}),
        );
        if has_item
            && !paused
            && matches!(status, PlaybackStatus::Playing | PlaybackStatus::Loading)
        {
            self.execute(ui, Action::TogglePlay);
            self.state.notice = "Audio output changed. Playback paused.".into();
        }
    }

    pub(super) fn handle_media_command(&mut self, ui: &AppWindow, command: RemoteCommand) {
        let label = match command {
            RemoteCommand::Play => "Play",
            RemoteCommand::Pause => "Pause",
            RemoteCommand::Toggle => "Play / Pause",
            RemoteCommand::Next => "Next Track",
            RemoteCommand::Previous => "Previous Track",
            RemoteCommand::SeekTo(_) => "Seek",
        };
        ui.set_last_media_command(
            format!(
                "{} · {}",
                self.text(label),
                chrono::Local::now().format("%H:%M:%S")
            )
            .into(),
        );
        let (has_item, paused, status) = self.active_transport();
        if !has_item {
            return;
        }
        match command {
            RemoteCommand::Play
                if paused || matches!(status, PlaybackStatus::Stopped | PlaybackStatus::Error) =>
            {
                self.execute(ui, Action::TogglePlay)
            }
            RemoteCommand::Pause
                if !paused
                    && matches!(status, PlaybackStatus::Playing | PlaybackStatus::Loading) =>
            {
                self.execute(ui, Action::TogglePlay)
            }
            RemoteCommand::Toggle => self.execute(ui, Action::TogglePlay),
            RemoteCommand::Next => self.execute(ui, Action::NextTrack),
            RemoteCommand::Previous => self.execute(ui, Action::PreviousTrack),
            RemoteCommand::SeekTo(seconds) => {
                let effect = self.state.seek(seconds);
                self.effects(ui, vec![effect]);
            }
            _ => {}
        }
    }

    pub(super) fn sync_device_status(&self, ui: &AppWindow) {
        let status = &self.device_status.snapshot;
        ui.set_battery_level(status.battery_percent.map_or(-1, i32::from));
        ui.set_battery_charging(status.charging);
        ui.set_battery_external_power(status.external_power);
        ui.set_audio_output_name(status.output.name.as_str().into());
        ui.set_audio_output_headphones(status.output.headphones);
        ui.set_audio_output_wireless(status.output.bluetooth);
        ui.set_native_settings_available(cfg!(target_os = "macos"));
        let battery = &status.accessory;
        ui.set_headphone_left_level(battery.left.map_or(-1, i32::from));
        ui.set_headphone_right_level(battery.right.map_or(-1, i32::from));
        ui.set_headphone_case_level(battery.case.map_or(-1, i32::from));
        ui.set_headphone_single_level(battery.single.map_or(-1, i32::from));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_pause_and_play_are_idempotent_and_output_loss_never_resumes() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        controller.state.current = Some(0);
        controller.state.status = PlaybackStatus::Playing;
        controller.state.paused = false;
        controller.handle_media_command(&ui, RemoteCommand::Pause);
        assert!(controller.state.paused);
        controller.state.podcasts.active = Some(1);
        controller.state.podcasts.paused = false;
        controller.state.podcasts.status = PlaybackStatus::Loading;
        controller.pause_for_output_change(&ui);
        assert!(controller.state.podcasts.paused);
        controller.state.podcasts.active = None;
        controller.state.video_active = true;
        controller.state.video_status = crate::video::Status::Playing;
        controller.state.video_paused = false;
        controller.pause_for_output_change(&ui);
        assert!(controller.state.video_paused);
        controller.state.video_active = false;
        controller.handle_media_command(&ui, RemoteCommand::Pause);
        assert!(controller.state.paused);
        controller.pause_for_output_change(&ui);
        assert!(controller.state.paused);
        controller.handle_media_command(&ui, RemoteCommand::Play);
        assert!(!controller.state.paused);
        controller.handle_media_command(&ui, RemoteCommand::Play);
        assert!(!controller.state.paused);
        controller.pause_for_output_change(&ui);
        assert!(controller.state.paused);
        assert!(ui.get_last_media_command().starts_with("Play"));
        controller.state.status = PlaybackStatus::Loading;
        controller.state.paused = false;
        controller.handle_media_command(&ui, RemoteCommand::Pause);
        assert!(controller.state.paused);
    }
}
