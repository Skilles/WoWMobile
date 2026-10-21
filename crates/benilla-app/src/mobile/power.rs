//! The mobile client's power use: a frame-rate limit (`mobileMaxFps`), so a phone renders no more
//! frames than its player asked for, and the device's thermal state, which the remote console
//! reports. Below the display's rate the limit is the event loop's own timed wait (bevy's reactive
//! update mode, woken by nothing else), so the main thread sleeps in the run loop between frames;
//! at the display's rate the display paces the loop, as it does any iOS app.

use std::time::Duration;

use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

/// The frame-rate limit's CVar.
pub(crate) const CVAR_MAX_FPS: &str = "mobileMaxFps";

/// The limits the Graphics page offers; the last is the display's own rate, no limit.
pub(crate) const MAX_FPS_CHOICES: [u32; 3] = [30, 40, 60];

/// The frame period `mobileMaxFps` asks for; `None` leaves the display's rate alone.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameCap(pub(crate) Option<Duration>);

impl Default for FrameCap {
    fn default() -> Self {
        Self(period(60))
    }
}

/// The period for `fps`; 0, or the fastest choice and past it, mean no limit.
fn period(fps: u32) -> Option<Duration> {
    (fps > 0 && fps < MAX_FPS_CHOICES[2]).then(|| Duration::from_secs_f64(1.0 / f64::from(fps)))
}

pub(crate) fn on_cvar(ev: On<crate::cvars::CvarChanged>, mut cap: ResMut<FrameCap>) {
    if ev.is(CVAR_MAX_FPS) {
        let fps = ev.num().max(0.0) as u32;
        let new = FrameCap(period(fps));
        if *cap != new {
            info!("mobile: frame rate limit {fps}");
            *cap = new;
        }
    }
}

/// The focused update mode the limit asks for. No event wakes a limited loop early: a touch waits
/// for the next frame, as it would at the display's rate, and a stream of touches cannot raise
/// the rate.
fn update_mode(cap: FrameCap) -> UpdateMode {
    match cap.0 {
        Some(wait) => UpdateMode::Reactive {
            wait,
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        },
        None => UpdateMode::Continuous,
    }
}

/// Put the limit into the event loop's settings when it changes.
pub(crate) fn apply_frame_cap(cap: Res<FrameCap>, settings: Option<ResMut<WinitSettings>>) {
    if !cap.is_changed() {
        return;
    }
    if let Some(mut settings) = settings {
        settings.focused_mode = update_mode(*cap);
    }
}

/// The device's thermal state as iOS reports it (`NSProcessInfoThermalState`): nominal, fair,
/// serious or critical. Elsewhere `None`.
pub(crate) fn thermal_state() -> Option<&'static str> {
    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState as T};
        let state = NSProcessInfo::processInfo().thermalState();
        Some(match state {
            T::Nominal => "nominal",
            T::Fair => "fair",
            T::Serious => "serious",
            T::Critical => "critical",
            _ => "unknown",
        })
    }
    #[cfg(not(target_os = "ios"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_rate_and_zero_mean_no_limit() {
        assert_eq!(period(60), None);
        assert_eq!(period(120), None);
        assert_eq!(period(0), None);
        assert_eq!(period(30), Some(Duration::from_secs_f64(1.0 / 30.0)));
        assert_eq!(FrameCap::default().0, None);
        assert_eq!(update_mode(FrameCap(None)), UpdateMode::Continuous);
        assert!(matches!(
            update_mode(FrameCap(period(30))),
            UpdateMode::Reactive {
                react_to_window_events: false,
                ..
            }
        ));
    }
}
