//! Touch controls (iOS): the screen's fingers become the mouse, the movement keys and a few
//! on-screen buttons, which the rest of benilla reads as it reads a keyboard and mouse. The
//! decisions are [`router`]'s; this module feeds it `TouchInput` and writes what it answers as
//! input messages ahead of `bevy_input`, and draws the joystick and the buttons.
//!
//! The buttons, down the right edge in the world: Jump (Space), Chat (Enter), Target (Tab), Menu
//! (Escape), the 1.12 default keys for `JUMP`, `OPENCHAT`, `TARGETNEARESTENEMY` and
//! `TOGGLEGAMEMENU`. The joystick holds W/S and Q/E, `MOVEFORWARD`/`MOVEBACKWARD` and the strafes,
//! so a look drag turns and the stick moves relative to the view.

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

/// A look drag's points to mouse motion: a finger covers far fewer points than a mouse does
/// pixels for the same turn.
const LOOK_SCALE: f32 = 2.5;

/// The on-screen buttons, top to bottom: the key each holds and its icon.
const BUTTONS: [(KeyCode, &str); 4] = [
    (KeyCode::Escape, "Interface\\Icons\\INV_Misc_Gear_01.blp"),
    (
        KeyCode::Tab,
        "Interface\\Icons\\Ability_Hunter_SniperShot.blp",
    ),
    (KeyCode::Enter, "Interface\\Icons\\INV_Letter_01.blp"),
    (KeyCode::Space, "Interface\\Icons\\Ability_Rogue_Sprint.blp"),
];
/// Button size, gap, and distance from the right edge (clear of the sensor housing in either
/// landscape), in points.
const BUTTON_SIZE: f32 = 52.0;
const BUTTON_GAP: f32 = 12.0;
const BUTTON_MARGIN: f32 = 64.0;

pub(crate) struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        // A touch screen is iOS; elsewhere the mouse and keyboard are real.
        if !cfg!(target_os = "ios") {
            return;
        }
        app.init_non_send_resource::<TouchState>()
            .add_systems(Startup, (spawn_overlay, defer_edge_gestures))
            .add_systems(PreUpdate, route_touches.before(InputSystems))
            .add_systems(Update, draw_overlay);
    }
}

/// The router and what this module last told the input layer.
#[derive(Default)]
struct TouchState {
    router: Router,
    keys: MoveKeys,
}

/// Whether the joystick and buttons are live: in the world, or anywhere under `WOW_TOUCH_ALWAYS`
/// (a check of the controls without a login).
fn controls_live(client: Option<&State<ClientState>>) -> bool {
    static ALWAYS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALWAYS.get_or_init(|| std::env::var_os("WOW_TOUCH_ALWAYS").is_some())
        || client.is_some_and(|s| *s.get() == ClientState::InWorld)
}

/// The buttons' rects in logical points for a window of `size`.
fn button_rects(size: Vec2) -> Vec<Rect> {
    let total = BUTTONS.len() as f32 * BUTTON_SIZE + (BUTTONS.len() - 1) as f32 * BUTTON_GAP;
    let top = (size.y - total) * 0.5;
    let left = size.x - BUTTON_MARGIN - BUTTON_SIZE;
    (0..BUTTONS.len())
        .map(|i| {
            let y = top + i as f32 * (BUTTON_SIZE + BUTTON_GAP);
            Rect::new(left, y, left + BUTTON_SIZE, y + BUTTON_SIZE)
        })
        .collect()
}

fn key_message(key: KeyCode, pressed: bool, window: Entity) -> KeyboardInput {
    let logical_key = match key {
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Enter => Key::Enter,
        KeyCode::Space => Key::Space,
        KeyCode::KeyW => Key::Character("w".into()),
        KeyCode::KeyS => Key::Character("s".into()),
        KeyCode::KeyQ => Key::Character("q".into()),
        KeyCode::KeyE => Key::Character("e".into()),
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

fn route_touches(
    mut state: NonSendMut<TouchState>,
    mut touches: MessageReader<TouchInput>,
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
    let batch: Vec<router::Touch> = touches
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
    let size = Vec2::new(window.width(), window.height());
    let ctx = Context {
        over_ui: over_ui.is_some_and(|o| o.0),
        in_world: controls_live(client.as_deref()),
        typing: script.is_some_and(|s| s.has_keyboard_focus()),
        window: size,
        buttons: button_rects(size),
    };
    let now = time.elapsed_secs();
    for output in state.router.update(&batch, now, &ctx) {
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
                    delta: d * LOOK_SCALE,
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
            Output::ButtonKey(i, pressed) => {
                keyboard.write(key_message(BUTTONS[i].0, pressed, window_entity));
            }
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
    state.keys = keys;
}

#[derive(Component)]
struct OverlayRoot;
#[derive(Component)]
struct OverlayButton(usize);
#[derive(Component)]
struct JoystickBase;
#[derive(Component)]
struct JoystickKnob;

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

fn spawn_overlay(mut commands: Commands, assets: Res<AssetServer>) {
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
            Visibility::Hidden,
        ))
        .with_children(|root| {
            for (i, (_, icon)) in BUTTONS.iter().enumerate() {
                let url = benilla_assets::texture_url(icon, (false, false));
                root.spawn((
                    OverlayButton(i),
                    circle(BUTTON_SIZE, Color::srgba(0.0, 0.0, 0.0, 0.35)),
                    Pickable::IGNORE,
                ))
                .with_children(|b| {
                    b.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        ImageNode {
                            image: assets.load(url),
                            color: Color::srgba(1.0, 1.0, 1.0, 0.85),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
            }
            root.spawn((
                JoystickBase,
                circle(112.0, Color::srgba(0.0, 0.0, 0.0, 0.25)),
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
            root.spawn((
                JoystickKnob,
                circle(48.0, Color::srgba(0.85, 0.75, 0.45, 0.45)),
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
        });
}

type OverlayParts<'w, 's> = (
    Query<'w, 's, (&'static mut Node, &'static mut Visibility), With<OverlayRoot>>,
    Query<
        'w,
        's,
        (&'static OverlayButton, &'static mut Node),
        (
            Without<OverlayRoot>,
            Without<JoystickBase>,
            Without<JoystickKnob>,
        ),
    >,
    Query<
        'w,
        's,
        (&'static mut Node, &'static mut Visibility),
        (
            With<JoystickBase>,
            Without<OverlayRoot>,
            Without<JoystickKnob>,
        ),
    >,
    Query<
        'w,
        's,
        (&'static mut Node, &'static mut Visibility),
        (
            With<JoystickKnob>,
            Without<OverlayRoot>,
            Without<JoystickBase>,
        ),
    >,
);

/// Show the overlay in the world, place the buttons for the window, and draw the joystick under
/// the finger that holds it.
fn draw_overlay(
    state: NonSend<TouchState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    client: Option<Res<State<ClientState>>>,
    mut parts: ParamSet<OverlayParts>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let in_world = controls_live(client.as_deref());
    for (_, mut vis) in parts.p0().iter_mut() {
        vis.set_if_neq(if in_world {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let rects = button_rects(size);
    for (button, mut node) in parts.p1().iter_mut() {
        let r = rects[button.0];
        node.left = Val::Px(r.min.x);
        node.top = Val::Px(r.min.y);
    }
    let stick = state.router.joystick().filter(|_| in_world);
    let radius = state.router.config.joystick_radius;
    for (mut node, mut vis) in parts.p2().iter_mut() {
        if let Some((center, _)) = stick {
            node.left = Val::Px(center.x - radius);
            node.top = Val::Px(center.y - radius);
            node.width = Val::Px(radius * 2.0);
            node.height = Val::Px(radius * 2.0);
            vis.set_if_neq(Visibility::Inherited);
        } else {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
    for (mut node, mut vis) in parts.p3().iter_mut() {
        if let Some((center, at)) = stick {
            let off = at - center;
            let off = off.clamp_length_max(radius);
            let knob = center + off;
            node.left = Val::Px(knob.x - 24.0);
            node.top = Val::Px(knob.y - 24.0);
            vis.set_if_neq(Visibility::Inherited);
        } else {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
}
