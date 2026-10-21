//! The mobile client's power use: a frame-rate limit (`mobileMaxFps`), so a phone renders no more
//! frames than its player asked for, and the device's thermal state, which the remote console
//! reports. The limit holds the main loop with a sleep, never a spin: a spinning core is the heat
//! the limit is for.

use std::time::{Duration, Instant};

use bevy::prelude::*;

/// The frame-rate limit's CVar.
pub(crate) const CVAR_MAX_FPS: &str = "mobileMaxFps";

/// The limits the Graphics page offers; 120 needs a ProMotion display, elsewhere the display's
/// own rate holds it lower.
pub(crate) const MAX_FPS_CHOICES: [u32; 4] = [30, 40, 60, 120];

/// The frame period `mobileMaxFps` asks for; `None` leaves the display's rate alone.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameCap(pub(crate) Option<Duration>);

impl Default for FrameCap {
    fn default() -> Self {
        Self(period(60))
    }
}

/// The period for `fps`, 0 or past the fastest choice meaning no limit.
fn period(fps: u32) -> Option<Duration> {
    (fps > 0 && fps < MAX_FPS_CHOICES[3]).then(|| Duration::from_secs_f64(1.0 / f64::from(fps)))
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

/// Sleep out the rest of the frame the limit allows. The last millisecond is not spun: a frame a
/// timer tick late costs nothing a phone's player sees, and a spin is heat.
pub(crate) fn cap_frame_rate(cap: Res<FrameCap>, mut last: Local<Option<Instant>>) {
    if let Some(period) = cap.0 {
        let wait = crate::video::pace_wait(*last, Instant::now(), period);
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
    *last = Some(Instant::now());
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
    fn the_fastest_choice_and_zero_mean_no_limit() {
        assert_eq!(period(120), None);
        assert_eq!(period(0), None);
        assert_eq!(period(30), Some(Duration::from_secs_f64(1.0 / 30.0)));
        assert_eq!(FrameCap::default().0, period(60));
    }
}
