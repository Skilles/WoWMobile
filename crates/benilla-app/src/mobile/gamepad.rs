//! Game controllers (Backbone One, Xbox, DualSense, any MFi pad) on the mobile client: the
//! `mobileGamepad*` CVars, read here.

use bevy::prelude::*;

/// The `mobileGamepad*` CVars.
#[derive(Resource, Clone, Debug)]
pub(crate) struct PadSettings {
    pub(crate) enabled: bool,
    pub(crate) look: f32,
    pub(crate) invert: bool,
    /// The serialized binding table (`mobileGamepadBindings`); empty is the default layout.
    pub(crate) bindings: String,
}

impl Default for PadSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            look: 1.0,
            invert: false,
            bindings: String::new(),
        }
    }
}

pub(crate) fn on_cvar(ev: On<crate::cvars::CvarChanged>, mut pad: ResMut<PadSettings>) {
    match ev.key().as_str() {
        "mobilegamepad" => pad.enabled = ev.flag(),
        "mobilegamepadlook" => pad.look = ev.num().clamp(0.1, 5.0),
        "mobilegamepadinvert" => pad.invert = ev.flag(),
        "mobilegamepadbindings" => pad.bindings = ev.new.clone(),
        _ => {}
    }
}
