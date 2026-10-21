//! Game controllers on the mobile client (Backbone One, Xbox, DualSense, any MFi pad), in the
//! layout WoW Forever plays with:
//!
//! - The left stick moves (W/S/Q/E, as the touch stick does); the right stick looks, holding the
//!   right mouse button so the character turns with the view.
//! - LB and RB cycle targets (`TargetNearestEnemy(1)` and `TargetNearestEnemy()`).
//! - LT, RT and both together switch the face buttons and the d-pad between four layers: 32
//!   bindings, each Jump, an action slot, a window or another command ([`Command`]), remappable
//!   through `mobileGamepadBindings` (the Controller Buttons screen, `MobilePad.xml`).
//! - Menu opens the radial menu; View toggles the game menu; L3 autoruns; R3 toggles the virtual
//!   cursor, which also opens by itself while a loot, vendor, quest or other such window is up:
//!   the right stick moves it, A left-clicks and B right-clicks.
//!
//! [`PadRouter`] is the mapping, plain state tested on any host; [`backend`] reads the iOS
//! GameController framework; the systems apply what the router answers as the same input messages
//! the touch layer writes, and run its commands in the interface's VM.

use bevy::input::mouse::{MouseButtonInput, MouseMotion};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow};

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

// ── The mapping ────────────────────────────────────────────────────────────────────────────────

/// One frame of a controller: sticks in [-1, 1] with y up, triggers in [0, 1].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PadState {
    pub(crate) left: Vec2,
    pub(crate) right: Vec2,
    pub(crate) lt: f32,
    pub(crate) rt: f32,
    pub(crate) lb: bool,
    pub(crate) rb: bool,
    /// A, B, X, Y, then the d-pad's up, right, down, left: [`FACE`] order.
    pub(crate) buttons: [bool; 8],
    pub(crate) menu: bool,
    pub(crate) options: bool,
    pub(crate) l3: bool,
    pub(crate) r3: bool,
}

/// The eight layered buttons' names, in [`PadState::buttons`] order.
pub(crate) const FACE: [char; 8] = ['A', 'B', 'X', 'Y', 'U', 'R', 'D', 'L'];

/// What a layered button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    None,
    Jump,
    /// An action slot, 1-120.
    Action(u8),
    VirtualCursor,
    Radial,
    /// A command of the mobile interface's table (`BENILLA_MOBILE_COMMAND`), by key.
    Ui(String),
}

impl Command {
    pub(crate) fn parse(text: &str) -> Command {
        let t = text.trim();
        match t {
            "" | "none" => Command::None,
            "jump" => Command::Jump,
            "cursor" => Command::VirtualCursor,
            "radial" => Command::Radial,
            _ => match t.strip_prefix('a').and_then(|n| n.parse::<u8>().ok()) {
                Some(n) if (1..=120).contains(&n) => Command::Action(n),
                _ if t.chars().all(|c| c.is_ascii_alphanumeric()) => Command::Ui(t.to_string()),
                _ => Command::None,
            },
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Command::None => "none".into(),
            Command::Jump => "jump".into(),
            Command::Action(n) => format!("a{n}"),
            Command::VirtualCursor => "cursor".into(),
            Command::Radial => "radial".into(),
            Command::Ui(k) => k.clone(),
        }
    }
}

/// The 32 layered bindings: `[layer][button]`, layer 0 none, 1 LT, 2 RT, 3 both.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Bindings(pub(crate) [[Command; 8]; 4]);

impl Default for Bindings {
    /// Jump on A, then action slots 1-31 in order down the layers.
    fn default() -> Self {
        let mut slot = 0u8;
        Bindings(std::array::from_fn(|layer| {
            std::array::from_fn(|b| {
                if layer == 0 && b == 0 {
                    Command::Jump
                } else {
                    slot += 1;
                    Command::Action(slot)
                }
            })
        }))
    }
}

impl Bindings {
    /// The defaults with `text`'s entries (`"<layer><button>=<command>;"`, e.g. `1X=map;`) on top.
    pub(crate) fn parse(text: &str) -> Bindings {
        let mut out = Bindings::default();
        for entry in text.split(';') {
            let Some((key, value)) = entry.split_once('=') else {
                continue;
            };
            let mut chars = key.trim().chars();
            let (Some(layer), Some(button), None) = (chars.next(), chars.next(), chars.next())
            else {
                continue;
            };
            let Some(layer) = layer.to_digit(10).filter(|l| *l < 4) else {
                continue;
            };
            let Some(b) = FACE.iter().position(|c| *c == button.to_ascii_uppercase()) else {
                continue;
            };
            out.0[layer as usize][b] = Command::parse(value);
        }
        out
    }

    pub(crate) fn serialize(&self) -> String {
        let mut out = String::new();
        for (layer, row) in self.0.iter().enumerate() {
            for (b, c) in row.iter().enumerate() {
                out.push_str(&format!("{layer}{}={};", FACE[b], c.text()));
            }
        }
        out
    }
}

/// What the router asks for this frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PadOutput {
    /// Relative mouse motion (look), in mouse pixels.
    Look(Vec2),
    /// Move the cursor by this many points.
    CursorBy(Vec2),
    Press(MouseButton),
    Release(MouseButton),
    /// Lua for the interface's VM.
    Lua(String),
}

/// What the rest of the app tells the router.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PadContext {
    pub(crate) in_world: bool,
    /// The interface wants a pointer (a loot, vendor, quest… window is up).
    pub(crate) wants_cursor: bool,
    pub(crate) look: f32,
    pub(crate) invert: bool,
}

/// Stick deflection under which a stick is at rest.
const DEADZONE: f32 = 0.2;
/// Deflection past which a stick axis holds its movement key.
const MOVE_THRESHOLD: f32 = 0.4;
/// A trigger is a layer shift past this.
const TRIGGER: f32 = 0.5;
/// Look speed at full deflection, mouse pixels a second.
const LOOK_SPEED: f32 = 900.0;
/// Virtual cursor speed at full deflection, points a second.
const CURSOR_SPEED: f32 = 700.0;
/// The right stick at rest this long ends a look.
const LOOK_RELEASE_SECS: f32 = 0.15;

#[derive(Default)]
pub(crate) struct PadRouter {
    pub(crate) bindings: Bindings,
    last: PadState,
    looking: bool,
    rest_since: Option<f32>,
    /// The cursor is on: toggled (R3, a binding) or because the interface wants one.
    toggled_cursor: bool,
    /// A held through the cursor (a left click in progress), and B likewise.
    clicking: [bool; 2],
}

impl PadRouter {
    /// The layer the triggers select.
    pub(crate) fn layer(state: &PadState) -> usize {
        match (state.lt > TRIGGER, state.rt > TRIGGER) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }

    pub(crate) fn cursor_on(&self, ctx: &PadContext) -> bool {
        self.toggled_cursor || ctx.wants_cursor || !ctx.in_world
    }

    /// The movement keys the left stick holds (forward, back, strafe left, strafe right).
    pub(crate) fn keys(state: &PadState) -> crate::touch::router::MoveKeys {
        let s = state.left;
        crate::touch::router::MoveKeys {
            forward: s.y > MOVE_THRESHOLD,
            back: s.y < -MOVE_THRESHOLD,
            left: s.x < -MOVE_THRESHOLD,
            right: s.x > MOVE_THRESHOLD,
        }
    }

    /// One frame of `state`, `dt` seconds after the last, at `now`.
    pub(crate) fn update(
        &mut self,
        state: &PadState,
        dt: f32,
        now: f32,
        ctx: &PadContext,
    ) -> Vec<PadOutput> {
        let mut out = Vec::new();
        let was = self.last;
        let pressed = |now: bool, then: bool| now && !then;

        if pressed(state.r3, was.r3) {
            self.toggled_cursor = !self.toggled_cursor;
        }
        let cursor = self.cursor_on(ctx);

        // The right stick: the cursor, or a look holding the right button.
        let mut r = state.right;
        if r.length() < DEADZONE {
            r = Vec2::ZERO;
        }
        if cursor {
            if self.looking {
                self.looking = false;
                out.push(PadOutput::Release(MouseButton::Right));
            }
            if r != Vec2::ZERO {
                out.push(PadOutput::CursorBy(
                    Vec2::new(r.x, -r.y) * CURSOR_SPEED * dt,
                ));
            }
        } else if r != Vec2::ZERO {
            self.rest_since = None;
            if !self.looking {
                self.looking = true;
                out.push(PadOutput::Press(MouseButton::Right));
            }
            let y = if ctx.invert { r.y } else { -r.y };
            out.push(PadOutput::Look(
                Vec2::new(r.x, y) * LOOK_SPEED * ctx.look * dt,
            ));
        } else if self.looking {
            let since = *self.rest_since.get_or_insert(now);
            if now - since >= LOOK_RELEASE_SECS {
                self.looking = false;
                self.rest_since = None;
                out.push(PadOutput::Release(MouseButton::Right));
            }
        }

        // Targets, the menus and autorun.
        if ctx.in_world {
            if pressed(state.lb, was.lb) {
                out.push(PadOutput::Lua("TargetNearestEnemy(1)".into()));
            }
            if pressed(state.rb, was.rb) {
                out.push(PadOutput::Lua("TargetNearestEnemy()".into()));
            }
            if pressed(state.l3, was.l3) {
                out.push(PadOutput::Lua("ToggleAutoRun()".into()));
            }
            if pressed(state.menu, was.menu) {
                out.push(PadOutput::Lua("BenillaMobileRadial_Toggle()".into()));
            }
        }
        if pressed(state.options, was.options) {
            out.push(PadOutput::Lua("ToggleGameMenu()".into()));
        }

        // The face buttons and the d-pad: clicks through the cursor (A, B), else the layer's.
        let layer = Self::layer(state);
        for b in 0..8 {
            let (now_down, then_down) = (state.buttons[b], was.buttons[b]);
            if cursor && b < 2 {
                let button = [MouseButton::Left, MouseButton::Right][b];
                if pressed(now_down, then_down) {
                    self.clicking[b] = true;
                    out.push(PadOutput::Press(button));
                } else if !now_down && self.clicking[b] {
                    self.clicking[b] = false;
                    out.push(PadOutput::Release(button));
                }
                continue;
            }
            if !pressed(now_down, then_down) || !ctx.in_world {
                continue;
            }
            match &self.bindings.0[layer][b] {
                Command::None => {}
                Command::Jump => out.push(PadOutput::Lua("Jump()".into())),
                Command::Action(n) => out.push(PadOutput::Lua(format!("UseAction({n})"))),
                Command::VirtualCursor => self.toggled_cursor = !self.toggled_cursor,
                Command::Radial => out.push(PadOutput::Lua("BenillaMobileRadial_Toggle()".into())),
                Command::Ui(key) => out.push(PadOutput::Lua(format!(
                    "if BenillaMobile_RunCommand then BenillaMobile_RunCommand(\"{key}\") end"
                ))),
            }
        }
        self.last = *state;
        out
    }

    /// The controller went away: let go of everything it held.
    pub(crate) fn release_all(&mut self) -> Vec<PadOutput> {
        let mut out = Vec::new();
        if self.looking {
            out.push(PadOutput::Release(MouseButton::Right));
        }
        for (b, held) in self.clicking.iter_mut().enumerate() {
            if *held {
                *held = false;
                out.push(PadOutput::Release(
                    [MouseButton::Left, MouseButton::Right][b],
                ));
            }
        }
        *self = PadRouter {
            bindings: std::mem::take(&mut self.bindings),
            ..Default::default()
        };
        out
    }
}

// ── The iOS GameController framework ───────────────────────────────────────────────────────────

#[cfg(target_os = "ios")]
pub(crate) mod backend {
    use objc2_game_controller::{GCController, GCControllerButtonInput, GCExtendedGamepad};

    use super::PadState;

    fn down(b: Option<objc2::rc::Retained<GCControllerButtonInput>>) -> bool {
        b.is_some_and(|b| unsafe { b.isPressed() })
    }

    /// The first connected controller with the extended profile, read now.
    pub(crate) fn poll() -> Option<PadState> {
        let controllers = unsafe { GCController::controllers() };
        let pad: objc2::rc::Retained<GCExtendedGamepad> = controllers
            .iter()
            .find_map(|c| unsafe { c.extendedGamepad() })?;
        unsafe {
            let stick =
                |s: objc2::rc::Retained<objc2_game_controller::GCControllerDirectionPad>| {
                    bevy::math::Vec2::new(s.xAxis().value(), s.yAxis().value())
                };
            let dpad = pad.dpad();
            Some(PadState {
                left: stick(pad.leftThumbstick()),
                right: stick(pad.rightThumbstick()),
                lt: pad.leftTrigger().value(),
                rt: pad.rightTrigger().value(),
                lb: pad.leftShoulder().isPressed(),
                rb: pad.rightShoulder().isPressed(),
                buttons: [
                    pad.buttonA().isPressed(),
                    pad.buttonB().isPressed(),
                    pad.buttonX().isPressed(),
                    pad.buttonY().isPressed(),
                    dpad.up().isPressed(),
                    dpad.right().isPressed(),
                    dpad.down().isPressed(),
                    dpad.left().isPressed(),
                ],
                menu: pad.buttonMenu().isPressed(),
                options: down(pad.buttonOptions()),
                l3: down(pad.leftThumbstickButton()),
                r3: down(pad.rightThumbstickButton()),
            })
        }
    }

    /// `$BENILLA_VIRTUAL_PAD`: Apple's on-screen controller, to try the controller path without a
    /// physical pad (the Simulator, or a phone with none paired). Kept alive for the run.
    pub(crate) fn connect_virtual() {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::{NSSet, NSString};
        use objc2_game_controller::{
            GCInputButtonA, GCInputButtonB, GCInputButtonX, GCInputButtonY, GCInputLeftShoulder,
            GCInputLeftThumbstick, GCInputLeftTrigger, GCInputRightShoulder,
            GCInputRightThumbstick, GCInputRightTrigger,
        };
        thread_local! {
            static KEEP: std::cell::RefCell<Option<Retained<AnyObject>>> =
                const { std::cell::RefCell::new(None) };
        }
        // The crate does not bind GCVirtualController (it lives beside UIKit), so by message; an
        // Objective-C exception is caught and logged rather than unwinding into Rust.
        let caught = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            let names: Vec<&NSString> = [
                GCInputButtonA,
                GCInputButtonB,
                GCInputButtonX,
                GCInputButtonY,
                // Not the d-pad: Apple's touch controller refuses it beside the left stick.
                GCInputLeftShoulder,
                GCInputRightShoulder,
                GCInputLeftTrigger,
                GCInputRightTrigger,
                GCInputLeftThumbstick,
                GCInputRightThumbstick,
            ]
            .into_iter()
            .flatten()
            .collect();
            let set = NSSet::from_slice(&names);
            let config: Retained<AnyObject> =
                msg_send![class!(GCVirtualControllerConfiguration), new];
            let _: () = msg_send![&config, setElements: &*set];
            let pad: Retained<AnyObject> = msg_send![class!(GCVirtualController), virtualControllerWithConfiguration: &*config];
            let _: () = msg_send![&pad, connectWithReplyHandler: std::ptr::null::<AnyObject>()];
            KEEP.with(|k| *k.borrow_mut() = Some(pad));
        }));
        match caught {
            Ok(()) => bevy::log::info!("gamepad: virtual controller up ($BENILLA_VIRTUAL_PAD)"),
            Err(e) => bevy::log::error!(
                "gamepad: the virtual controller raised: {:?}",
                e.map(|e| e.to_string())
            ),
        }
    }
}

#[cfg(not(target_os = "ios"))]
pub(crate) mod backend {
    pub(crate) fn poll() -> Option<super::PadState> {
        None
    }

    pub(crate) fn connect_virtual() {}
}

// ── The systems ────────────────────────────────────────────────────────────────────────────────

/// The router and what it last told the input layer.
#[derive(Default)]
pub(crate) struct PadRuntime {
    router: PadRouter,
    keys: crate::touch::router::MoveKeys,
    connected: bool,
    bindings_text: Option<String>,
    /// The interface's last "wants a pointer" answer, and when it was asked.
    wants_cursor: bool,
    asked_at: f32,
}

/// Lua the router asked for, waiting for the system that holds the VM.
#[derive(Resource, Default)]
pub(crate) struct PadLua {
    run: Vec<String>,
    /// What the interface shows of the pad: connected, layer, cursor on.
    publish: Option<(bool, usize, bool)>,
    published: Option<(bool, usize, bool)>,
    /// The effective binding table, for the Controller Buttons screen, when it changed.
    bindings: Option<String>,
    /// The VM last published to ([`benilla_ui::script::UiScript::session`]).
    session: u64,
}

/// On the main thread (the `NonSend` pins it there): the virtual controller is a UIKit view.
pub(crate) fn start_virtual_pad(_main_thread: NonSend<PadRuntime>) {
    if std::env::var_os("BENILLA_VIRTUAL_PAD").is_some() {
        backend::connect_virtual();
    }
}

type PadWriters<'w> = (
    MessageWriter<'w, CursorMoved>,
    MessageWriter<'w, MouseButtonInput>,
    MessageWriter<'w, MouseMotion>,
    MessageWriter<'w, bevy::input::keyboard::KeyboardInput>,
);

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_pad(
    mut rt: NonSendMut<PadRuntime>,
    settings: Res<PadSettings>,
    mut lua: ResMut<PadLua>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    time: Res<Time>,
    client: Option<Res<State<crate::client_state::ClientState>>>,
    mut writers: PadWriters,
) {
    let Ok((window_entity, mut window)) = windows.single_mut() else {
        return;
    };
    let (cursor_moved, mouse_buttons, motion, keyboard) = &mut writers;
    if rt.bindings_text.as_deref() != Some(settings.bindings.as_str()) {
        rt.router.bindings = Bindings::parse(&settings.bindings);
        rt.bindings_text = Some(settings.bindings.clone());
        lua.bindings = Some(rt.router.bindings.serialize());
    }
    let state = if settings.enabled {
        backend::poll()
    } else {
        None
    };
    let in_world = client.is_some_and(|s| *s.get() == crate::client_state::ClientState::InWorld);
    let ctx = PadContext {
        in_world,
        wants_cursor: rt.wants_cursor,
        look: settings.look,
        invert: settings.invert,
    };
    let outputs = match state {
        Some(state) => {
            if !rt.connected {
                info!("gamepad: connected");
                rt.connected = true;
            }
            let out = rt
                .router
                .update(&state, time.delta_secs(), time.elapsed_secs(), &ctx);
            let keys = if in_world {
                PadRouter::keys(&state)
            } else {
                Default::default()
            };
            for (held, was, key) in [
                (keys.forward, rt.keys.forward, KeyCode::KeyW),
                (keys.back, rt.keys.back, KeyCode::KeyS),
                (keys.left, rt.keys.left, KeyCode::KeyQ),
                (keys.right, rt.keys.right, KeyCode::KeyE),
            ] {
                if held != was {
                    keyboard.write(crate::touch::key_message(key, held, window_entity));
                }
            }
            rt.keys = keys;
            lua.publish = Some((true, PadRouter::layer(&state), rt.router.cursor_on(&ctx)));
            out
        }
        None if rt.connected => {
            info!("gamepad: disconnected");
            rt.connected = false;
            for (was, key) in [
                (rt.keys.forward, KeyCode::KeyW),
                (rt.keys.back, KeyCode::KeyS),
                (rt.keys.left, KeyCode::KeyQ),
                (rt.keys.right, KeyCode::KeyE),
            ] {
                if was {
                    keyboard.write(crate::touch::key_message(key, false, window_entity));
                }
            }
            rt.keys = Default::default();
            lua.publish = Some((false, 0, false));
            rt.router.release_all()
        }
        None => return,
    };
    for o in outputs {
        match o {
            PadOutput::Look(d) => {
                motion.write(MouseMotion { delta: d });
            }
            PadOutput::CursorBy(d) => {
                let size = Vec2::new(window.width(), window.height());
                let at = window.cursor_position().unwrap_or(size * 0.5);
                let to = (at + d).clamp(Vec2::ZERO, size);
                window.set_cursor_position(Some(to));
                cursor_moved.write(CursorMoved {
                    window: window_entity,
                    position: to,
                    delta: Some(to - at),
                });
            }
            PadOutput::Press(b) | PadOutput::Release(b) => {
                // A click lands where the cursor is: put it there first, as a mouse is.
                if window.cursor_position().is_none() {
                    let c = Vec2::new(window.width(), window.height()) * 0.5;
                    window.set_cursor_position(Some(c));
                    cursor_moved.write(CursorMoved {
                        window: window_entity,
                        position: c,
                        delta: None,
                    });
                }
                mouse_buttons.write(MouseButtonInput {
                    button: b,
                    state: if matches!(o, PadOutput::Press(_)) {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                    window: window_entity,
                });
            }
            PadOutput::Lua(code) => lua.run.push(code),
        }
    }
}

/// Run the pad's Lua, tell the interface its state, and ask it whether it wants a pointer.
pub(crate) fn serve_pad_lua(
    mut lua: ResMut<PadLua>,
    mut rt: NonSendMut<PadRuntime>,
    time: Res<Time>,
    mut vm: crate::script_calls::HostedVm,
) {
    let run = std::mem::take(&mut lua.run);
    let ask = rt.connected && time.elapsed_secs() - rt.asked_at > 0.25;
    let mut publish_table = lua.bindings.clone();
    let mut publish_state = lua.publish.filter(|p| lua.published != Some(*p));
    let seen_session = lua.session;
    let answer = vm.run(|script| {
        let script = script?;
        // A new VM (the world's, a reload) has none of it yet.
        let session = script.session();
        if session != seen_session {
            publish_table = Some(rt.router.bindings.serialize());
            publish_state = lua.publish;
        }
        for code in &run {
            if let Err(e) = script.run(code) {
                warn!("gamepad: {code}: {e}");
            }
        }
        // The table's text is `<digit><letter>=<word>;`, nothing a Lua string must escape.
        if let Some(table) = &publish_table {
            let _ = script.run(&format!("BENILLA_PAD_BINDINGS = \"{table}\""));
        }
        if let Some((connected, layer, cursor)) = publish_state {
            let _ = script.run(&format!(
                "BENILLA_PAD_CONNECTED = {}; BENILLA_PAD_LAYER = {layer}; BENILLA_PAD_CURSOR = {}",
                if connected { "1" } else { "nil" },
                if cursor { "1" } else { "nil" }
            ));
        }
        let wants = ask.then(|| {
            script
                .eval::<bool>(
                    "return BenillaMobilePad_WantsCursor ~= nil and BenillaMobilePad_WantsCursor() ~= nil",
                )
                .unwrap_or(false)
        });
        Some((session, wants))
    });
    // Nothing reached a VM (none is up): keep it all for the next frame.
    let Some((session, wants)) = answer else {
        lua.run = run;
        return;
    };
    lua.session = session;
    lua.bindings = None;
    if publish_state.is_some() {
        lua.published = publish_state;
    }
    if let Some(w) = wants {
        rt.wants_cursor = w;
        rt.asked_at = time.elapsed_secs();
    }
}

// The tests step one pad state frame by frame, as a controller does.
#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn world() -> PadContext {
        PadContext {
            in_world: true,
            wants_cursor: false,
            look: 1.0,
            invert: false,
        }
    }

    #[test]
    fn the_default_layout_is_jump_then_slots_1_to_31() {
        let b = Bindings::default();
        assert_eq!(b.0[0][0], Command::Jump);
        assert_eq!(b.0[0][1], Command::Action(1));
        assert_eq!(b.0[1][0], Command::Action(8));
        assert_eq!(b.0[3][7], Command::Action(31));
    }

    #[test]
    fn bindings_round_trip_and_override_the_defaults() {
        let b = Bindings::parse("1X=map;0B=a42;2U=cursor;3L=none;bogus;9A=jump;");
        assert_eq!(b.0[1][2], Command::Ui("map".into()));
        assert_eq!(b.0[0][1], Command::Action(42));
        assert_eq!(b.0[2][4], Command::VirtualCursor);
        assert_eq!(b.0[3][7], Command::None);
        assert_eq!(
            b.0[0][0],
            Command::Jump,
            "untouched entries keep the default"
        );
        assert_eq!(Bindings::parse(&b.serialize()), b);
    }

    #[test]
    fn the_triggers_pick_the_layer() {
        let mut s = PadState::default();
        assert_eq!(PadRouter::layer(&s), 0);
        s.lt = 0.9;
        assert_eq!(PadRouter::layer(&s), 1);
        s.rt = 0.9;
        assert_eq!(PadRouter::layer(&s), 3);
        s.lt = 0.1;
        assert_eq!(PadRouter::layer(&s), 2);
    }

    #[test]
    fn a_face_button_fires_its_layers_binding_once_per_press() {
        let mut r = PadRouter::default();
        let mut s = PadState::default();
        s.buttons[2] = true; // X, layer 0: slot 2
        assert_eq!(
            r.update(&s, 0.016, 0.0, &world()),
            vec![PadOutput::Lua("UseAction(2)".into())]
        );
        assert!(
            r.update(&s, 0.016, 0.02, &world()).is_empty(),
            "held is not pressed again"
        );
        s.buttons[2] = false;
        r.update(&s, 0.016, 0.04, &world());
        s.lt = 1.0;
        s.buttons[2] = true; // LT + X: slot 10
        assert_eq!(
            r.update(&s, 0.016, 0.06, &world()),
            vec![PadOutput::Lua("UseAction(10)".into())]
        );
    }

    #[test]
    fn the_right_stick_looks_with_the_right_button_held() {
        let mut r = PadRouter::default();
        let mut s = PadState::default();
        s.right = Vec2::new(1.0, 0.0);
        let out = r.update(&s, 0.1, 0.0, &world());
        assert_eq!(out[0], PadOutput::Press(MouseButton::Right));
        assert_eq!(out[1], PadOutput::Look(Vec2::new(90.0, 0.0)));
        s.right = Vec2::ZERO;
        assert!(
            r.update(&s, 0.1, 0.1, &world()).is_empty(),
            "a moment at rest keeps the look"
        );
        assert_eq!(
            r.update(&s, 0.1, 0.3, &world()),
            vec![PadOutput::Release(MouseButton::Right)]
        );
    }

    #[test]
    fn a_window_that_wants_a_pointer_turns_the_stick_into_a_cursor() {
        let mut r = PadRouter::default();
        let mut s = PadState::default();
        let ctx = PadContext {
            wants_cursor: true,
            ..world()
        };
        s.right = Vec2::new(0.0, 1.0);
        assert_eq!(
            r.update(&s, 0.1, 0.0, &ctx),
            vec![PadOutput::CursorBy(Vec2::new(0.0, -70.0))]
        );
        s.right = Vec2::ZERO;
        s.buttons[0] = true; // A clicks rather than jumps
        assert_eq!(
            r.update(&s, 0.1, 0.1, &ctx),
            vec![PadOutput::Press(MouseButton::Left)]
        );
        s.buttons[0] = false;
        assert_eq!(
            r.update(&s, 0.1, 0.2, &ctx),
            vec![PadOutput::Release(MouseButton::Left)]
        );
    }

    #[test]
    fn r3_toggles_the_cursor_and_lb_rb_cycle_targets() {
        let mut r = PadRouter::default();
        let mut s = PadState::default();
        s.r3 = true;
        r.update(&s, 0.016, 0.0, &world());
        assert!(r.cursor_on(&world()));
        s.r3 = false;
        s.rb = true;
        assert_eq!(
            r.update(&s, 0.016, 0.02, &world()),
            vec![PadOutput::Lua("TargetNearestEnemy()".into())]
        );
        s.rb = false;
        s.lb = true;
        assert_eq!(
            r.update(&s, 0.016, 0.04, &world()),
            vec![PadOutput::Lua("TargetNearestEnemy(1)".into())]
        );
    }

    #[test]
    fn the_left_stick_holds_the_movement_keys() {
        let mut s = PadState::default();
        s.left = Vec2::new(0.7, 0.9);
        let k = PadRouter::keys(&s);
        assert!(k.forward && k.right && !k.back && !k.left);
    }

    #[test]
    fn a_lost_controller_lets_go() {
        let mut r = PadRouter::default();
        let mut s = PadState::default();
        s.right = Vec2::new(1.0, 0.0);
        r.update(&s, 0.1, 0.0, &world());
        assert_eq!(
            r.release_all(),
            vec![PadOutput::Release(MouseButton::Right)]
        );
        assert!(r.release_all().is_empty());
    }
}
