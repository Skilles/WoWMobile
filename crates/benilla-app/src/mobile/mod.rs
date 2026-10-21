//! The mobile fork: what benilla adds to play on a phone. Its interface lives in `assets/mobile`
//! (loaded by `ui_script::mobile_ui` after benilla's layer), its settings are `mobile*` CVars on the
//! options window's Mobile page, and its input is [`crate::touch`].

pub(crate) mod console;
pub(crate) mod gamepad;
pub(crate) mod launcher;

/// `assets/mobile`'s manifest for the in-game interface.
pub(crate) const WORLD_MANIFEST: &str = "mobile.toc";
/// `assets/mobile`'s manifest for the glue screens.
pub(crate) const GLUE_MANIFEST: &str = "glue.toc";

/// Whether this run is the mobile client: always on iOS, and on a desktop under `BENILLA_MOBILE`,
/// to work on the mobile interface with a mouse.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| cfg!(target_os = "ios") || std::env::var_os("BENILLA_MOBILE").is_some())
}

pub(crate) struct MobilePlugin;

impl bevy::app::Plugin for MobilePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        use bevy::prelude::*;
        if !enabled() {
            return;
        }
        app.init_resource::<gamepad::PadSettings>()
            .add_observer(gamepad::on_cvar)
            .add_observer(launcher::on_cvar)
            .add_systems(
                Update,
                launcher::remember_on_roster.after(crate::glue_session::login::LoginPolicy),
            );
    }
}
