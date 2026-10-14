use nano_music_player::ui::{AppWindow, LibraryRow};
use slint::{ComponentHandle, ModelRc, VecModel};
fn main() {
    #[cfg(target_os = "macos")]
    if std::env::var_os("SLINT_BACKEND").is_none() {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia".into())
            .select()
            .unwrap();
    }
    let ui = AppWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(274., 540.));
    ui.set_screen(1);
    ui.set_screen_title("Native Font Audit".into());
    ui.set_selected(-1);
    ui.set_smooth_slides(false);
    ui.set_rows(ModelRc::new(VecModel::from(
        ["Helvetica Neue", "Helvetica", "Arial"]
            .map(|family| LibraryRow {
                title: format!("{family} · Genius").into(),
                subtitle: "Create from local songs".into(),
                font_family: family.into(),
                ..Default::default()
            })
            .to_vec(),
    )));
    ui.run().unwrap();
}
