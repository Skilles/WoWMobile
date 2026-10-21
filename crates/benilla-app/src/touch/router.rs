//! What each finger is doing, as plain state: the touch half of [`super`], free of Bevy so it is
//! tested on any host. Fed one frame's touches, it answers the mouse, wheel and key input they
//! stand for.
//!
//! - A finger that lands on the left of the screen in the world, off any UI frame, is the
//!   joystick: its offset from where it landed holds the movement keys.
//! - A finger on an on-screen button holds that button's key.
//! - Any other finger is the mouse. A tap clicks left where it lifted; holding still is a right
//!   click; a drag over UI is a left drag, and over the world a right-button mouse-look, which
//!   steers the character as a right-drag does.
//! - A second mouse finger turns both into a pinch, which zooms like the wheel.

use bevy::math::{Rect, Vec2};
use bevy::platform::collections::HashMap;

/// Tunables, in logical points and seconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Config {
    /// A mouse finger that moves farther than this is a drag, not a tap.
    pub(crate) slop: f32,
    /// A mouse finger held still this long is a right click.
    pub(crate) hold_secs: f32,
    /// The joystick takes fingers landing left of this fraction of the window width.
    pub(crate) joystick_zone: f32,
    /// The joystick's travel: an offset this long is full deflection.
    pub(crate) joystick_radius: f32,
    /// The deflection, as a fraction of the radius, past which an axis holds its key.
    pub(crate) joystick_threshold: f32,
    /// Points of pinch spread per wheel notch.
    pub(crate) pinch_per_notch: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            slop: 12.0,
            hold_secs: 0.5,
            joystick_zone: 0.42,
            joystick_radius: 56.0,
            joystick_threshold: 0.35,
            pinch_per_notch: 36.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Touch {
    pub(crate) id: u64,
    pub(crate) phase: Phase,
    /// Logical points, y down from the window's top left.
    pub(crate) pos: Vec2,
}

/// What the rest of the app tells the router about this frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct Context {
    /// The cursor is over a UI frame (what the cursor showed last frame).
    pub(crate) over_ui: bool,
    /// In the world: the joystick and the buttons are live.
    pub(crate) in_world: bool,
    /// An EditBox has the keyboard: the joystick holds no keys.
    pub(crate) typing: bool,
    pub(crate) window: Vec2,
    /// The on-screen buttons' rects, in the order their keys are named.
    pub(crate) buttons: Vec<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    Left,
    Right,
}

/// The movement keys the joystick holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MoveKeys {
    pub(crate) forward: bool,
    pub(crate) back: bool,
    pub(crate) left: bool,
    pub(crate) right: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Output {
    /// Put the cursor here.
    Cursor(Vec2),
    Press(Button),
    Release(Button),
    /// Relative mouse motion, in points.
    Look(Vec2),
    /// Wheel notches, positive away from the user (zoom in).
    Wheel(f32),
    /// On-screen button `i` went down or up.
    ButtonKey(usize, bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Down, not yet a tap, a hold or a drag.
    Pending,
    /// A left drag over UI.
    UiDrag,
    /// A right-button mouse-look over the world.
    Look,
    /// Already a right click; nothing more until it lifts.
    Held,
}

#[derive(Clone, Copy, Debug)]
enum Role {
    /// Landed in the joystick zone while no mouse finger was down: the cursor went there to ask
    /// the UI whether a frame is under it, and the answer comes next frame.
    Probe {
        start: Vec2,
        last: Vec2,
        since: f32,
    },
    Joystick {
        center: Vec2,
        at: Vec2,
    },
    Mouse {
        start: Vec2,
        last: Vec2,
        since: f32,
        mode: Mode,
        /// Whether a UI frame was under the finger where it landed; known from its second frame.
        over_ui: Option<bool>,
    },
    Pinch {
        last: Vec2,
    },
    Button(usize),
    /// Out of a pinch; ignored until it lifts.
    Spent,
}

#[derive(Default)]
pub(crate) struct Router {
    pub(crate) config: Config,
    roles: HashMap<u64, Role>,
    /// Touches that started this frame: their first frame's UI answer is not in yet.
    fresh: Vec<u64>,
    /// Output held back a frame, so a press and its release never land in one frame.
    deferred: Vec<Output>,
    keys: MoveKeys,
}

impl Router {
    /// The joystick's centre and finger, while one is down (for drawing it).
    pub(crate) fn joystick(&self) -> Option<(Vec2, Vec2)> {
        self.roles.values().find_map(|r| match *r {
            Role::Joystick { center, at } => Some((center, at)),
            _ => None,
        })
    }

    /// The movement keys held after the last [`Self::update`].
    pub(crate) fn keys(&self) -> MoveKeys {
        self.keys
    }

    fn mouse_down(&self) -> bool {
        self.roles
            .values()
            .any(|r| matches!(r, Role::Mouse { .. } | Role::Probe { .. }))
    }

    /// One frame: `touches` in arrival order, `now` in seconds.
    pub(crate) fn update(&mut self, touches: &[Touch], now: f32, ctx: &Context) -> Vec<Output> {
        let mut out = std::mem::take(&mut self.deferred);

        // Last frame's fresh touches now have the UI's answer for where they landed.
        for id in std::mem::take(&mut self.fresh) {
            match self.roles.get(&id).copied() {
                Some(Role::Probe { start, last, .. }) => {
                    let role = if ctx.over_ui {
                        Role::Mouse {
                            start,
                            last,
                            since: now,
                            mode: Mode::Pending,
                            over_ui: Some(true),
                        }
                    } else {
                        Role::Joystick {
                            center: start,
                            at: last,
                        }
                    };
                    self.roles.insert(id, role);
                }
                Some(Role::Mouse {
                    start,
                    last,
                    since,
                    mode,
                    over_ui: None,
                }) => {
                    self.roles.insert(
                        id,
                        Role::Mouse {
                            start,
                            last,
                            since,
                            mode,
                            over_ui: Some(ctx.over_ui),
                        },
                    );
                }
                _ => {}
            }
        }

        for t in touches {
            match t.phase {
                Phase::Start => self.start(t, now, ctx, &mut out),
                Phase::Move => self.moved(t, ctx, &mut out),
                Phase::End | Phase::Cancel => self.end(t, &mut out),
            }
        }

        // A mouse finger held still long enough is a right click where it landed.
        let cfg = self.config;
        for role in self.roles.values_mut() {
            if let Role::Mouse { since, mode, .. } = role {
                if *mode == Mode::Pending && now - *since >= cfg.hold_secs {
                    *mode = Mode::Held;
                    out.push(Output::Press(Button::Right));
                    self.deferred.push(Output::Release(Button::Right));
                }
            }
        }

        self.keys = match self.joystick() {
            Some((center, at)) if ctx.in_world && !ctx.typing => {
                let v = (at - center) / cfg.joystick_radius;
                let t = cfg.joystick_threshold;
                MoveKeys {
                    forward: v.y < -t,
                    back: v.y > t,
                    left: v.x < -t,
                    right: v.x > t,
                }
            }
            _ => MoveKeys::default(),
        };
        out
    }

    fn start(&mut self, t: &Touch, now: f32, ctx: &Context, out: &mut Vec<Output>) {
        if ctx.in_world {
            if let Some(i) = ctx.buttons.iter().position(|r| r.contains(t.pos)) {
                self.roles.insert(t.id, Role::Button(i));
                out.push(Output::ButtonKey(i, true));
                return;
            }
        }
        // A second mouse finger: both become a pinch, and the first lets go of what it held.
        let mouse = self.roles.iter().find_map(|(id, r)| match *r {
            Role::Mouse { last, mode, .. } => Some((*id, last, mode)),
            _ => None,
        });
        let joystick_zone = ctx.in_world
            && !ctx.typing
            && t.pos.x < ctx.window.x * self.config.joystick_zone
            && self.joystick().is_none();
        if let Some((first, last, mode)) = mouse.filter(|_| !joystick_zone) {
            match mode {
                Mode::UiDrag => out.push(Output::Release(Button::Left)),
                Mode::Look => out.push(Output::Release(Button::Right)),
                Mode::Pending | Mode::Held => {}
            }
            self.roles.insert(first, Role::Pinch { last });
            self.roles.insert(t.id, Role::Pinch { last: t.pos });
            return;
        }
        if joystick_zone {
            if self.mouse_down() {
                // The cursor is busy with another finger: a finger here is the joystick.
                self.roles.insert(
                    t.id,
                    Role::Joystick {
                        center: t.pos,
                        at: t.pos,
                    },
                );
            } else {
                self.roles.insert(
                    t.id,
                    Role::Probe {
                        start: t.pos,
                        last: t.pos,
                        since: now,
                    },
                );
                self.fresh.push(t.id);
                out.push(Output::Cursor(t.pos));
            }
            return;
        }
        self.roles.insert(
            t.id,
            Role::Mouse {
                start: t.pos,
                last: t.pos,
                since: now,
                mode: Mode::Pending,
                over_ui: None,
            },
        );
        self.fresh.push(t.id);
        out.push(Output::Cursor(t.pos));
    }

    fn moved(&mut self, t: &Touch, ctx: &Context, out: &mut Vec<Output>) {
        let cfg = self.config;
        let Some(role) = self.roles.get(&t.id).copied() else {
            return;
        };
        match role {
            Role::Probe { start, since, .. } => {
                self.roles.insert(
                    t.id,
                    Role::Probe {
                        start,
                        last: t.pos,
                        since,
                    },
                );
            }
            Role::Joystick { mut center, .. } => {
                // Past full travel the base follows the finger, so reversing is immediate.
                let off = t.pos - center;
                let len = off.length();
                if len > cfg.joystick_radius {
                    center += off / len * (len - cfg.joystick_radius);
                }
                self.roles
                    .insert(t.id, Role::Joystick { center, at: t.pos });
            }
            Role::Mouse {
                start,
                last,
                since,
                mode,
                over_ui,
            } => {
                let mode = match mode {
                    Mode::Pending if t.pos.distance(start) > cfg.slop => {
                        if over_ui.unwrap_or(ctx.over_ui) {
                            out.push(Output::Press(Button::Left));
                            out.push(Output::Cursor(t.pos));
                            Mode::UiDrag
                        } else {
                            out.push(Output::Press(Button::Right));
                            out.push(Output::Look(t.pos - last));
                            Mode::Look
                        }
                    }
                    Mode::UiDrag => {
                        out.push(Output::Cursor(t.pos));
                        Mode::UiDrag
                    }
                    Mode::Look => {
                        out.push(Output::Look(t.pos - last));
                        Mode::Look
                    }
                    m => m,
                };
                self.roles.insert(
                    t.id,
                    Role::Mouse {
                        start,
                        last: t.pos,
                        since,
                        mode,
                        over_ui,
                    },
                );
            }
            Role::Pinch { last } => {
                if let Some(other) = self.roles.iter().find_map(|(id, r)| match *r {
                    Role::Pinch { last } if *id != t.id => Some(last),
                    _ => None,
                }) {
                    let before = last.distance(other);
                    let after = t.pos.distance(other);
                    out.push(Output::Wheel((after - before) / cfg.pinch_per_notch));
                }
                self.roles.insert(t.id, Role::Pinch { last: t.pos });
            }
            Role::Button(_) | Role::Spent => {}
        }
    }

    fn end(&mut self, t: &Touch, out: &mut Vec<Output>) {
        self.fresh.retain(|id| *id != t.id);
        let cancel = t.phase == Phase::Cancel;
        match self.roles.remove(&t.id) {
            // A tap in the joystick zone, too quick to have been asked about: a click.
            Some(Role::Probe { .. })
            | Some(Role::Mouse {
                mode: Mode::Pending,
                ..
            }) if !cancel => {
                out.push(Output::Press(Button::Left));
                self.deferred.push(Output::Release(Button::Left));
            }
            Some(Role::Mouse {
                mode: Mode::UiDrag, ..
            }) => {
                if !cancel {
                    out.push(Output::Cursor(t.pos));
                }
                out.push(Output::Release(Button::Left));
            }
            Some(Role::Mouse {
                mode: Mode::Look, ..
            }) => out.push(Output::Release(Button::Right)),
            Some(Role::Pinch { .. }) => {
                // The other pinch finger is spent until it lifts.
                for role in self.roles.values_mut() {
                    if matches!(role, Role::Pinch { .. }) {
                        *role = Role::Spent;
                    }
                }
            }
            Some(Role::Button(i)) => out.push(Output::ButtonKey(i, false)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(over_ui: bool) -> Context {
        Context {
            over_ui,
            in_world: true,
            typing: false,
            window: Vec2::new(956.0, 440.0),
            buttons: vec![Rect::new(850.0, 300.0, 900.0, 350.0)],
        }
    }

    fn t(id: u64, phase: Phase, x: f32, y: f32) -> Touch {
        Touch {
            id,
            phase,
            pos: Vec2::new(x, y),
        }
    }

    #[test]
    fn a_tap_clicks_left_and_releases_a_frame_later() {
        let mut r = Router::default();
        let c = ctx(false);
        assert_eq!(
            r.update(&[t(1, Phase::Start, 600.0, 200.0)], 0.0, &c),
            vec![Output::Cursor(Vec2::new(600.0, 200.0))]
        );
        assert_eq!(
            r.update(&[t(1, Phase::End, 600.0, 200.0)], 0.1, &c),
            vec![Output::Press(Button::Left)]
        );
        assert_eq!(r.update(&[], 0.12, &c), vec![Output::Release(Button::Left)]);
    }

    #[test]
    fn holding_still_is_a_right_click() {
        let mut r = Router::default();
        let c = ctx(false);
        r.update(&[t(1, Phase::Start, 600.0, 200.0)], 0.0, &c);
        assert!(r.update(&[], 0.3, &c).is_empty());
        assert_eq!(r.update(&[], 0.6, &c), vec![Output::Press(Button::Right)]);
        assert_eq!(
            r.update(&[], 0.62, &c),
            vec![Output::Release(Button::Right)]
        );
        assert!(r
            .update(&[t(1, Phase::End, 600.0, 200.0)], 0.9, &c)
            .is_empty());
    }

    #[test]
    fn a_drag_over_the_world_is_a_right_button_look() {
        let mut r = Router::default();
        let c = ctx(false);
        r.update(&[t(1, Phase::Start, 600.0, 200.0)], 0.0, &c);
        r.update(&[], 0.02, &c);
        assert_eq!(
            r.update(&[t(1, Phase::Move, 620.0, 200.0)], 0.04, &c),
            vec![
                Output::Press(Button::Right),
                Output::Look(Vec2::new(20.0, 0.0))
            ]
        );
        assert_eq!(
            r.update(&[t(1, Phase::Move, 625.0, 203.0)], 0.06, &c),
            vec![Output::Look(Vec2::new(5.0, 3.0))]
        );
        assert_eq!(
            r.update(&[t(1, Phase::End, 625.0, 203.0)], 0.08, &c),
            vec![Output::Release(Button::Right)]
        );
    }

    #[test]
    fn a_drag_over_ui_is_a_left_drag() {
        let mut r = Router::default();
        r.update(&[t(1, Phase::Start, 600.0, 400.0)], 0.0, &ctx(true));
        r.update(&[], 0.02, &ctx(true));
        assert_eq!(
            r.update(&[t(1, Phase::Move, 640.0, 400.0)], 0.04, &ctx(true)),
            vec![
                Output::Press(Button::Left),
                Output::Cursor(Vec2::new(640.0, 400.0))
            ]
        );
        assert_eq!(
            r.update(&[t(1, Phase::End, 650.0, 400.0)], 0.06, &ctx(true)),
            vec![
                Output::Cursor(Vec2::new(650.0, 400.0)),
                Output::Release(Button::Left)
            ]
        );
    }

    #[test]
    fn a_finger_on_the_left_off_ui_is_the_joystick() {
        let mut r = Router::default();
        let c = ctx(false);
        assert_eq!(
            r.update(&[t(1, Phase::Start, 150.0, 300.0)], 0.0, &c),
            vec![Output::Cursor(Vec2::new(150.0, 300.0))]
        );
        assert!(r
            .update(&[t(1, Phase::Move, 150.0, 250.0)], 0.02, &c)
            .is_empty());
        assert_eq!(
            r.keys(),
            MoveKeys {
                forward: true,
                ..Default::default()
            }
        );
        // A long hold is no right click for the joystick.
        assert!(r
            .update(&[t(1, Phase::Move, 200.0, 250.0)], 1.0, &c)
            .is_empty());
        assert_eq!(
            r.keys(),
            MoveKeys {
                forward: true,
                right: true,
                ..Default::default()
            }
        );
        r.update(&[t(1, Phase::End, 200.0, 250.0)], 1.1, &c);
        assert_eq!(r.keys(), MoveKeys::default());
    }

    #[test]
    fn a_finger_on_the_left_over_ui_is_the_mouse() {
        let mut r = Router::default();
        r.update(&[t(1, Phase::Start, 150.0, 300.0)], 0.0, &ctx(false));
        r.update(&[], 0.02, &ctx(true));
        assert!(r.joystick().is_none());
        assert_eq!(
            r.update(&[t(1, Phase::End, 150.0, 300.0)], 0.1, &ctx(true)),
            vec![Output::Press(Button::Left)]
        );
    }

    #[test]
    fn the_joystick_and_a_look_drag_work_together() {
        let mut r = Router::default();
        let c = ctx(false);
        r.update(&[t(1, Phase::Start, 600.0, 200.0)], 0.0, &c);
        r.update(&[], 0.02, &c);
        r.update(&[t(1, Phase::Move, 630.0, 200.0)], 0.04, &c);
        // The cursor is busy, so the left finger is the joystick at once.
        assert!(r
            .update(&[t(2, Phase::Start, 150.0, 300.0)], 0.05, &c)
            .is_empty());
        r.update(&[t(2, Phase::Move, 150.0, 240.0)], 0.06, &c);
        assert!(r.keys().forward);
        assert_eq!(
            r.update(&[t(1, Phase::Move, 640.0, 200.0)], 0.07, &c),
            vec![Output::Look(Vec2::new(10.0, 0.0))]
        );
    }

    #[test]
    fn two_mouse_fingers_pinch_to_zoom() {
        let mut r = Router::default();
        let c = ctx(false);
        r.update(&[t(1, Phase::Start, 600.0, 200.0)], 0.0, &c);
        assert!(r
            .update(&[t(2, Phase::Start, 700.0, 200.0)], 0.02, &c)
            .is_empty());
        assert_eq!(
            r.update(&[t(2, Phase::Move, 736.0, 200.0)], 0.04, &c),
            vec![Output::Wheel(1.0)]
        );
        r.update(&[t(1, Phase::End, 600.0, 200.0)], 0.06, &c);
        // The finger left over moves nothing.
        assert!(r
            .update(&[t(2, Phase::Move, 760.0, 200.0)], 0.08, &c)
            .is_empty());
        assert!(r
            .update(&[t(2, Phase::End, 760.0, 200.0)], 0.1, &c)
            .is_empty());
    }

    #[test]
    fn a_button_holds_its_key() {
        let mut r = Router::default();
        let c = ctx(false);
        assert_eq!(
            r.update(&[t(1, Phase::Start, 875.0, 325.0)], 0.0, &c),
            vec![Output::ButtonKey(0, true)]
        );
        assert!(r.update(&[], 1.0, &c).is_empty());
        assert_eq!(
            r.update(&[t(1, Phase::End, 875.0, 325.0)], 1.1, &c),
            vec![Output::ButtonKey(0, false)]
        );
    }

    #[test]
    fn off_the_world_there_is_no_joystick_and_no_buttons() {
        let mut r = Router::default();
        let c = Context {
            in_world: false,
            ..ctx(false)
        };
        r.update(&[t(1, Phase::Start, 150.0, 300.0)], 0.0, &c);
        r.update(&[t(1, Phase::Move, 150.0, 250.0)], 0.02, &c);
        assert!(r.joystick().is_none());
        assert_eq!(r.keys(), MoveKeys::default());
        let mut r = Router::default();
        assert_eq!(
            r.update(&[t(2, Phase::Start, 875.0, 325.0)], 0.0, &c),
            vec![Output::Cursor(Vec2::new(875.0, 325.0))]
        );
    }

    #[test]
    fn typing_holds_no_movement_keys() {
        let mut r = Router::default();
        r.update(&[t(1, Phase::Start, 150.0, 300.0)], 0.0, &ctx(false));
        r.update(&[t(1, Phase::Move, 150.0, 240.0)], 0.02, &ctx(false));
        assert!(r.keys().forward);
        let typing = Context {
            typing: true,
            ..ctx(false)
        };
        r.update(&[], 0.04, &typing);
        assert_eq!(r.keys(), MoveKeys::default());
    }
}
