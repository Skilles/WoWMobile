//! Touch input (the mobile client): the screen's fingers become the mouse, the movement keys and the
//! wheel, which the rest of benilla reads as it reads a keyboard and mouse. The decisions are
//! [`router`]'s; this module feeds it `TouchInput` (and [`InjectedTouches`], the remote console's)
//! and writes what it answers as input messages ahead of `bevy_input`, and draws the movement stick
//! while a thumb holds it. The jump ring, the radial menu and the views are the mobile interface's
//! (`assets/mobile`), driven by the mouse this module makes.
//!
//! The stick holds W/S and Q/E, `MOVEFORWARD`/`MOVEBACKWARD` and the strafes, so a look drag turns
//! and the stick moves relative to the view. The `mobile*` CVars tune it (`MobileOptions.xml`).

pub(crate) mod router;

use benilla_ui::script::UiScript;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow, ScreenEdge};

use crate::client_state::ClientState;
use router::{Button, Context, MoveKeys, Output, Phase, Router};

/// A look drag's points to mouse motion at `mobileLookSpeed` 1: a finger covers far fewer points
/// than a mouse does pixels for the same turn.
const LOOK_SCALE: f32 = 2.5;
/// The stick's travel at `mobileStickSize` 1, in points.
const STICK_RADIUS: f32 = 56.0;
/// The knob's diameter, in points.
const KNOB: f32 = 48.0;

pub(crate) struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InjectedTouches>();
        if !crate::mobile::enabled() {
            return;
        }
        app.init_non_send_resource::<TouchState>()
            .init_resource::<TouchSettings>()
            .add_observer(on_cvar)
            .add_systems(Startup, (spawn_overlay, defer_edge_gestures))
            .add_systems(PreUpdate, route_touches.before(InputSystems))
            // Where UI nodes are placed for this frame's layout, apart from the world's systems.
            .add_systems(PostUpdate, draw_overlay.before(bevy::ui::UiSystems::Layout));
    }
}

/// Touches from somewhere other than the screen (the remote console), routed with the screen's at
/// the next frame, in logical points.
#[derive(Resource, Default)]
pub(crate) struct InjectedTouches(pub(crate) Vec<router::Touch>);

/// The `mobile*` CVars this module reads.
#[derive(Resource, Clone, Copy, Debug)]
struct TouchSettings {
    controls: bool,
    look: f32,
    hold: f32,
    stick: f32,
    zone: f32,
    alpha: f32,
}

impl Default for TouchSettings {
    fn default() -> Self {
        Self {
            controls: true,
            look: 1.0,
            hold: 0.5,
            stick: 1.0,
            zone: 0.42,
            alpha: 0.4,
        }
    }
}

fn on_cvar(ev: On<crate::cvars::CvarChanged>, mut settings: ResMut<TouchSettings>) {
    match ev.key().as_str() {
        "mobiletouchcontrols" => settings.controls = ev.flag(),
        "mobilelookspeed" => settings.look = ev.num().clamp(0.1, 5.0),
        "mobileholdtime" => settings.hold = ev.num().clamp(0.15, 2.0),
        "mobilesticksize" => settings.stick = ev.num().clamp(0.3, 3.0),
        "mobilestickzone" => settings.zone = ev.num().clamp(0.1, 0.9),
        "mobilecontrolsalpha" => settings.alpha = ev.num().clamp(0.05, 1.0),
        _ => {}
    }
}

/// The router and what this module last told the input layer.
#[derive(Default)]
struct TouchState {
    router: Router,
    keys: MoveKeys,
}

/// Whether the stick is live: in the world with touch controls on, or anywhere under
/// `WOW_TOUCH_ALWAYS` (a check of the controls without a login).
fn controls_live(client: Option<&State<ClientState>>, settings: &TouchSettings) -> bool {
    static ALWAYS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALWAYS.get_or_init(|| std::env::var_os("WOW_TOUCH_ALWAYS").is_some())
        || (settings.controls && client.is_some_and(|s| *s.get() == ClientState::InWorld))
}

/// A key's press or release as a keyboard would send it.
pub(crate) fn key_message(key: KeyCode, pressed: bool, window: Entity) -> KeyboardInput {
    let logical_key = match key {
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Enter => Key::Enter,
        KeyCode::Space => Key::Space,
        KeyCode::KeyW => Key::Character("w".into()),
        KeyCode::KeyS => Key::Character("s".into()),
        KeyCode::KeyQ => Key::Character("q".into()),
        KeyCode::KeyE => Key::Character("e".into()),
        KeyCode::KeyA => Key::Character("a".into()),
        KeyCode::KeyD => Key::Character("d".into()),
        _ => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
    };
    KeyboardInput {
        key_code: key,
        logical_key,
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        text: None,
        repeat: false,
        window,
    }
}

/// Ask iOS to let a swipe from any edge reach the game first: the home and Control Center swipes
/// then take a second swipe, so a thumb near an edge does not leave the game.
fn defer_edge_gestures(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = windows.single_mut() {
        window.prefers_home_indicator_hidden = true;
        window.preferred_screen_edges_deferring_system_gestures = ScreenEdge::All;
    }
}

/// The input messages a touch can stand for.
type InputWriters<'w> = (
    MessageWriter<'w, CursorMoved>,
    MessageWriter<'w, MouseButtonInput>,
    MessageWriter<'w, MouseMotion>,
    MessageWriter<'w, MouseWheel>,
    MessageWriter<'w, KeyboardInput>,
);

#[allow(clippy::too_many_arguments)]
fn route_touches(
    mut state: NonSendMut<TouchState>,
    mut touches: MessageReader<TouchInput>,
    mut injected: ResMut<InjectedTouches>,
    settings: Res<TouchSettings>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    time: Res<Time>,
    over_ui: Option<Res<crate::ui_script::PointerOverUi>>,
    client: Option<Res<State<ClientState>>>,
    script: Option<NonSend<UiScript>>,
    mut writers: InputWriters,
) {
    let Ok((window_entity, mut window)) = windows.single_mut() else {
        return;
    };
    let (cursor_moved, mouse_buttons, motion, wheel, keyboard) = &mut writers;
    let mut batch: Vec<router::Touch> = touches
        .read()
        .map(|t| router::Touch {
            id: t.id,
            phase: match t.phase {
                TouchPhase::Started => Phase::Start,
                TouchPhase::Moved => Phase::Move,
                TouchPhase::Ended => Phase::End,
                TouchPhase::Canceled => Phase::Cancel,
            },
            pos: t.position,
        })
        .collect();
    batch.append(&mut injected.0);
    let size = Vec2::new(window.width(), window.height());
    state.router.config.hold_secs = settings.hold;
    state.router.config.joystick_radius = STICK_RADIUS * settings.stick;
    state.router.config.joystick_zone = settings.zone;
    // The interface says what the hovered frame is, asked only while fingers are landing or
    // lifting: a Lua call a frame would cost every frame for nothing.
    let kind = if batch.is_empty() && !state.router.awaits_ui() {
        router::Kind::Plain
    } else {
        script.as_deref().map_or(router::Kind::Plain, touch_kind)
    };
    let ctx = Context {
        over_ui: over_ui.is_some_and(|o| o.0),
        kind,
        in_world: controls_live(client.as_deref(), &settings),
        typing: script.is_some_and(|s| s.has_keyboard_focus()),
        window: size,
        buttons: Vec::new(),
    };
    let now = time.elapsed_secs();
    for output in state.router.update(&batch, now, &ctx) {
        if !matches!(output, Output::Cursor(_) | Output::Look(_)) {
            debug!("touch: {output:?}");
        }
        match output {
            Output::Cursor(p) => {
                window.set_cursor_position(Some(p));
                cursor_moved.write(CursorMoved {
                    window: window_entity,
                    position: p,
                    delta: None,
                });
            }
            Output::Press(b) | Output::Release(b) => {
                mouse_buttons.write(MouseButtonInput {
                    button: match b {
                        Button::Left => MouseButton::Left,
                        Button::Right => MouseButton::Right,
                    },
                    state: if matches!(output, Output::Press(_)) {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                    window: window_entity,
                });
            }
            Output::Look(d) => {
                motion.write(MouseMotion {
                    delta: d * LOOK_SCALE * settings.look,
                });
            }
            Output::Wheel(y) => {
                wheel.write(MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    y,
                    window: window_entity,
                });
            }
            // No on-screen buttons of the router's own: the mobile interface draws them.
            Output::ButtonKey(..) => {}
        }
    }
    let keys = state.router.keys();
    for (held, was, key) in [
        (keys.forward, state.keys.forward, KeyCode::KeyW),
        (keys.back, state.keys.back, KeyCode::KeyS),
        (keys.left, state.keys.left, KeyCode::KeyQ),
        (keys.right, state.keys.right, KeyCode::KeyE),
    ] {
        if held != was {
            keyboard.write(key_message(key, held, window_entity));
        }
    }
    if keys != state.keys {
        debug!("touch: movement keys {keys:?}");
    }
    state.keys = keys;
}

/// `BenillaMobile_TouchKind()` (`assets/mobile/MobileControls.xml`): what the frame under the
/// cursor is. The glue screens have no such function, and every frame there is plain.
fn touch_kind(script: &UiScript) -> router::Kind {
    let answer = script.eval::<Option<String>>(
        "return BenillaMobile_TouchKind and BenillaMobile_TouchKind() or nil",
    );
    match answer.ok().flatten().as_deref() {
        Some("item") => router::Kind::Item,
        Some("spell") => router::Kind::Spell,
        Some("action") => router::Kind::Action,
        _ => router::Kind::Plain,
    }
}

#[derive(Component)]
struct OverlayRoot;
#[derive(Component)]
struct StickBase;
#[derive(Component)]
struct StickKnob;

fn circle(size: f32, color: Color) -> (Node, BackgroundColor, BorderColor) {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(Color::srgba(0.85, 0.75, 0.45, 0.6)),
    )
}

fn spawn_overlay(mut commands: Commands) {
    commands
        .spawn((
            OverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // Over the game's UI, under the loading screen (1000) and the fade (1050).
            GlobalZIndex(900),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                StickBase,
                circle(STICK_RADIUS * 2.0, Color::srgba(0.0, 0.0, 0.0, 0.25)),
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
            root.spawn((
                StickKnob,
                circle(KNOB, Color::srgba(0.85, 0.75, 0.45, 0.45)),
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
        });
}

type StickParts<'w, 's> = (
    Query<
        'w,
        's,
        (
            &'static mut Node,
            &'static mut Visibility,
            &'static mut BackgroundColor,
        ),
        (With<StickBase>, Without<StickKnob>),
    >,
    Query<
        'w,
        's,
        (
            &'static mut Node,
            &'static mut Visibility,
            &'static mut BackgroundColor,
        ),
        (With<StickKnob>, Without<StickBase>),
    >,
);

/// Draw the stick under the thumb that holds it; nothing while no thumb does.
fn draw_overlay(
    state: NonSend<TouchState>,
    settings: Res<TouchSettings>,
    client: Option<Res<State<ClientState>>>,
    mut parts: ParamSet<StickParts>,
) {
    let live = controls_live(client.as_deref(), &settings);
    let stick = state.router.joystick().filter(|_| live);
    let radius = state.router.config.joystick_radius;
    for (mut node, mut vis, mut color) in parts.p0().iter_mut() {
        if let Some((center, _)) = stick {
            node.left = Val::Px(center.x - radius);
            node.top = Val::Px(center.y - radius);
            node.width = Val::Px(radius * 2.0);
            node.height = Val::Px(radius * 2.0);
            color.0 = Color::srgba(0.0, 0.0, 0.0, 0.3 * settings.alpha);
            vis.set_if_neq(Visibility::Inherited);
        } else {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
    for (mut node, mut vis, mut color) in parts.p1().iter_mut() {
        if let Some((center, at)) = stick {
            let knob = center + (at - center).clamp_length_max(radius);
            node.left = Val::Px(knob.x - KNOB * 0.5);
            node.top = Val::Px(knob.y - KNOB * 0.5);
            color.0 = Color::srgba(0.85, 0.75, 0.45, 0.55 * settings.alpha);
            vis.set_if_neq(Visibility::Inherited);
        } else {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
}
