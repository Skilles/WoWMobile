//! The remote console: a debugging port that drives the running client from another machine on
//! the network, one command a line, one JSON reply a line. Off unless `mobileRemoteConsole` is 1
//! or `$BENILLA_CONSOLE` is set; every connection first sends `auth <token>`, the token being
//! `benilla-config/console-token` (made on first use) or `$BENILLA_CONSOLE_TOKEN`.
//!
//! Commands (points are the window's logical points, y down):
//!
//! - `lua <chunk>`: run Lua in the interface's VM, as the interface's own code runs; `lua =<expr>`
//!   answers the expression's values.
//! - `tap x y`, `hold x y secs`, `drag x1 y1 x2 y2 secs`, `pinch x y from to secs`: touches,
//!   through the same router as the screen's.
//! - `key <KeyCode> [down|up]`: a key, `tap` by default (`key Space`, `key KeyW down`);
//!   `type <text>`: text, as the on-screen keyboard sends it.
//! - `shot`: a screenshot to `benilla-config/Screenshots/console-<n>.png`; the reply names it.
//! - `console <line>`: one `/console` line; `state`: where the client is; `size`: the window;
//!   `focus`: the EditBox holding the keyboard; `state` also gives the last frame's time and,
//!   on iOS, the thermal state; `log [text]`: the recent log lines (the crash
//!   report's ring, so `RUST_LOG` at launch decides what is in it), those holding `text` if given;
//!   `target`: the selected unit's guid, NPC flags and quest-giver status.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::PrimaryWindow;
use crossbeam_channel::{Receiver, Sender};

use crate::touch::router::{Phase, Touch};
use crate::touch::InjectedTouches;

/// The console's port.
pub(crate) const PORT: u16 = 7979;

pub(crate) struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        // Where it can be switched on: the mobile client (its options page), or a desktop run
        // that asks for it.
        if !crate::mobile::enabled() && std::env::var_os("BENILLA_CONSOLE").is_none() {
            return;
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        app.insert_resource(Inbox {
            tx,
            rx,
            started: false,
        })
        .init_resource::<Gestures>()
        .init_resource::<LuaQueue>()
        .add_observer(on_cvar)
        .add_systems(Startup, start_from_env)
        // Ahead of the touch router (`PreUpdate`), which takes them.
        .add_systems(First, release_gestures)
        // The whole world, so out of `Update`'s ordering; its Lua waits for the next drain.
        .add_systems(Last, serve_world)
        .add_systems(Update, serve_lua.in_set(crate::ui_script::UiDrain));
    }
}

/// One command off the wire, and where its reply goes.
struct Request {
    line: String,
    reply: Sender<String>,
}

#[derive(Resource)]
struct Inbox {
    tx: Sender<Request>,
    rx: Receiver<Request>,
    started: bool,
}

/// Lua commands, waiting for the system that holds the VM.
#[derive(Resource, Default)]
struct LuaQueue(Vec<Request>);

/// Touches a gesture command scheduled, each due at a `Time::elapsed_secs`.
#[derive(Resource, Default)]
struct Gestures {
    due: Vec<(f32, Touch)>,
    next_id: u64,
}

impl Gestures {
    /// A finger id no screen touch uses.
    fn finger(&mut self) -> u64 {
        self.next_id += 1;
        (1 << 48) + self.next_id
    }

    fn path(&mut self, now: f32, points: &[Vec2], secs: f32, hold_at_start: f32) {
        let id = self.finger();
        let Some((&first, rest)) = points.split_first() else {
            return;
        };
        self.due.push((
            now,
            Touch {
                id,
                phase: Phase::Start,
                pos: first,
            },
        ));
        let mut t = now + hold_at_start;
        let steps = rest.len().max(1) as f32;
        for &p in rest {
            t += secs / steps;
            self.due.push((
                t,
                Touch {
                    id,
                    phase: Phase::Move,
                    pos: p,
                },
            ));
        }
        let last = *points.last().unwrap_or(&first);
        self.due.push((
            t.max(now + hold_at_start) + 0.02,
            Touch {
                id,
                phase: Phase::End,
                pos: last,
            },
        ));
    }
}

fn reply(ok: bool, out: impl Into<String>) -> String {
    serde_json::json!({ "ok": ok, "out": out.into() }).to_string()
}

fn on_cvar(ev: On<crate::cvars::CvarChanged>, mut inbox: ResMut<Inbox>) {
    if ev.is("mobileRemoteConsole") && ev.flag() {
        start(&mut inbox);
    }
}

fn start_from_env(mut inbox: ResMut<Inbox>) {
    if std::env::var_os("BENILLA_CONSOLE").is_some() {
        start(&mut inbox);
    }
}

/// The token a connection must send first.
fn token() -> Option<String> {
    if let Ok(t) = std::env::var("BENILLA_CONSOLE_TOKEN") {
        return Some(t);
    }
    let path = crate::local_state::home()?.join("console-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    let t: String = {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..24)
            .map(|_| char::from(b"abcdefghjkmnpqrstuvwxyz23456789"[rng.gen_range(0..31)]))
            .collect()
    };
    let _ = std::fs::create_dir_all(path.parent()?);
    std::fs::write(&path, &t).ok()?;
    Some(t)
}

fn start(inbox: &mut Inbox) {
    if inbox.started {
        return;
    }
    let Some(token) = token() else {
        warn!("console: no benilla-config folder for the token — not listening");
        return;
    };
    let listener =
        match TcpListener::bind(("::", PORT)).or_else(|_| TcpListener::bind(("0.0.0.0", PORT))) {
            Ok(l) => l,
            Err(e) => {
                warn!("console: cannot listen on port {PORT}: {e}");
                return;
            }
        };
    inbox.started = true;
    info!("console: listening on port {PORT}");
    let tx = inbox.tx.clone();
    std::thread::Builder::new()
        .name("benilla-console".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let token = token.clone();
                let _ = std::thread::Builder::new()
                    .name("benilla-console-conn".into())
                    .spawn(move || serve_connection(stream, &token, &tx));
            }
        })
        .ok();
}

fn serve_connection(stream: TcpStream, token: &str, tx: &Sender<Request>) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let reader = BufReader::new(stream);
    let mut authed = false;
    for line in reader.lines() {
        let Ok(line) = line else {
            return;
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let answer = if !authed {
            if line.strip_prefix("auth ").map(str::trim) == Some(token) {
                authed = true;
                reply(true, "authenticated")
            } else {
                let _ = writeln!(out, "{}", reply(false, "send `auth <token>` first"));
                return;
            }
        } else {
            let (rtx, rrx) = crossbeam_channel::bounded(1);
            if tx.send(Request { line, reply: rtx }).is_err() {
                return;
            }
            rrx.recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| reply(false, "timed out (is the app in front?)"))
        };
        if writeln!(out, "{answer}").is_err() {
            return;
        }
    }
}

/// Every command but Lua, with the whole world.
fn serve_world(world: &mut World) {
    let requests: Vec<Request> = world.resource::<Inbox>().rx.try_iter().collect();
    for req in requests {
        let (verb, rest) = req
            .line
            .split_once(' ')
            .map_or((req.line.as_str(), ""), |(v, r)| (v, r.trim()));
        let answer = match verb {
            "lua" | "focus" => {
                world.resource_mut::<LuaQueue>().0.push(req);
                continue;
            }
            "tap" | "hold" | "drag" | "pinch" => gesture(world, verb, rest),
            "key" => key(world, rest),
            "type" => type_text(world, rest),
            "shot" => shot(world),
            "console" => reply(true, crate::console::execute(world, rest).join("\n")),
            "state" => state(world),
            "size" => size(world),
            "log" => recent_log(rest),
            "target" => target(world),
            _ => reply(false, format!("unknown command `{verb}`")),
        };
        let _ = req.reply.send(answer);
    }
}

fn numbers(rest: &str) -> Vec<f32> {
    rest.split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect()
}

fn gesture(world: &mut World, verb: &str, rest: &str) -> String {
    let n = numbers(rest);
    let now = world.resource::<Time>().elapsed_secs();
    let mut g = world.resource_mut::<Gestures>();
    match (verb, n.as_slice()) {
        ("tap", [x, y]) => g.path(now, &[Vec2::new(*x, *y)], 0.0, 0.05),
        ("hold", [x, y, secs]) => g.path(now, &[Vec2::new(*x, *y)], 0.0, *secs),
        ("drag", [x1, y1, x2, y2, secs]) => {
            let (a, b) = (Vec2::new(*x1, *y1), Vec2::new(*x2, *y2));
            let points: Vec<Vec2> = (0..=12).map(|i| a.lerp(b, i as f32 / 12.0)).collect();
            g.path(now, &points, *secs, 0.0);
        }
        ("pinch", [x, y, from, to, secs]) => {
            let c = Vec2::new(*x, *y);
            for dir in [-1.0, 1.0] {
                let points: Vec<Vec2> = (0..=12)
                    .map(|i| {
                        let r = from + (to - from) * i as f32 / 12.0;
                        c + Vec2::new(dir * r * 0.5, 0.0)
                    })
                    .collect();
                g.path(now, &points, *secs, 0.0);
            }
        }
        _ => return reply(false, format!("`{verb}` wants numbers: {rest}")),
    }
    reply(true, "scheduled")
}

fn release_gestures(
    time: Res<Time>,
    mut g: ResMut<Gestures>,
    mut injected: ResMut<InjectedTouches>,
) {
    let now = time.elapsed_secs();
    let mut due: Vec<(f32, Touch)> = Vec::new();
    g.due.retain(|(t, touch)| {
        if *t <= now {
            due.push((*t, *touch));
            false
        } else {
            true
        }
    });
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    injected.0.extend(due.into_iter().map(|(_, t)| t));
}

fn key(world: &mut World, rest: &str) -> String {
    let mut words = rest.split_whitespace();
    let Some(name) = words.next() else {
        return reply(false, "key <KeyCode> [down|up]");
    };
    let Some(code) = key_code(name) else {
        return reply(false, format!("no key `{name}`"));
    };
    let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    else {
        return reply(false, "no window");
    };
    let states: &[bool] = match words.next() {
        Some("down") => &[true],
        Some("up") => &[false],
        _ => &[true, false],
    };
    for &down in states {
        world.write_message(crate::touch::key_message(code, down, window));
    }
    reply(true, "sent")
}

/// Each character of `text` as a key press carrying it, as the on-screen keyboard sends text.
fn type_text(world: &mut World, text: &str) -> String {
    use bevy::input::keyboard::{Key, KeyboardInput};
    let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    else {
        return reply(false, "no window");
    };
    for c in text.chars() {
        for down in [true, false] {
            world.write_message(KeyboardInput {
                key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
                logical_key: Key::Character(c.to_string().into()),
                state: if down {
                    bevy::input::ButtonState::Pressed
                } else {
                    bevy::input::ButtonState::Released
                },
                text: down.then(|| c.to_string().into()),
                repeat: false,
                window,
            });
        }
    }
    reply(true, format!("typed {} characters", text.chars().count()))
}

fn key_code(name: &str) -> Option<KeyCode> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "space" => KeyCode::Space,
        "enter" | "return" => KeyCode::Enter,
        "escape" | "esc" => KeyCode::Escape,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "shift" => KeyCode::ShiftLeft,
        "ctrl" => KeyCode::ControlLeft,
        "alt" => KeyCode::AltLeft,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        _ => {
            let c = n.strip_prefix("key").unwrap_or(&n);
            if let Some(d) = n.strip_prefix("digit").or_else(|| {
                (n.len() == 1 && n.chars().all(|c| c.is_ascii_digit())).then_some(n.as_str())
            }) {
                return Some(match d {
                    "0" => KeyCode::Digit0,
                    "1" => KeyCode::Digit1,
                    "2" => KeyCode::Digit2,
                    "3" => KeyCode::Digit3,
                    "4" => KeyCode::Digit4,
                    "5" => KeyCode::Digit5,
                    "6" => KeyCode::Digit6,
                    "7" => KeyCode::Digit7,
                    "8" => KeyCode::Digit8,
                    "9" => KeyCode::Digit9,
                    _ => return None,
                });
            }
            if let Some(f) = n.strip_prefix('f').and_then(|d| d.parse::<u8>().ok()) {
                return Some(match f {
                    1 => KeyCode::F1,
                    2 => KeyCode::F2,
                    3 => KeyCode::F3,
                    4 => KeyCode::F4,
                    5 => KeyCode::F5,
                    6 => KeyCode::F6,
                    7 => KeyCode::F7,
                    8 => KeyCode::F8,
                    9 => KeyCode::F9,
                    10 => KeyCode::F10,
                    11 => KeyCode::F11,
                    12 => KeyCode::F12,
                    _ => return None,
                });
            }
            let mut chars = c.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                return None;
            };
            match ch {
                'a' => KeyCode::KeyA,
                'b' => KeyCode::KeyB,
                'c' => KeyCode::KeyC,
                'd' => KeyCode::KeyD,
                'e' => KeyCode::KeyE,
                'f' => KeyCode::KeyF,
                'g' => KeyCode::KeyG,
                'h' => KeyCode::KeyH,
                'i' => KeyCode::KeyI,
                'j' => KeyCode::KeyJ,
                'k' => KeyCode::KeyK,
                'l' => KeyCode::KeyL,
                'm' => KeyCode::KeyM,
                'n' => KeyCode::KeyN,
                'o' => KeyCode::KeyO,
                'p' => KeyCode::KeyP,
                'q' => KeyCode::KeyQ,
                'r' => KeyCode::KeyR,
                's' => KeyCode::KeyS,
                't' => KeyCode::KeyT,
                'u' => KeyCode::KeyU,
                'v' => KeyCode::KeyV,
                'w' => KeyCode::KeyW,
                'x' => KeyCode::KeyX,
                'y' => KeyCode::KeyY,
                'z' => KeyCode::KeyZ,
                _ => return None,
            }
        }
    })
}

fn target(world: &mut World) -> String {
    let selection = world.resource::<crate::target::Selection>();
    let (Some(entity), Some(guid)) = (selection.target, selection.guid) else {
        return reply(true, "no target");
    };
    let flags = world
        .get::<crate::net::ObjectStore>(entity)
        .map(|s| s.0.unit_npc_flags());
    let status = world
        .get_resource::<crate::ui_quest::QuestGiver>()
        .and_then(|q| q.status(guid));
    reply(
        true,
        format!("guid={guid:#x} npc_flags={flags:?} quest_status={status:?}"),
    )
}

fn recent_log(filter: &str) -> String {
    let lines: Vec<String> = benilla_world::log_ring::recent()
        .into_iter()
        .filter(|l| filter.is_empty() || l.contains(filter))
        .collect();
    reply(true, lines.join("\n"))
}

fn shot(world: &mut World) -> String {
    let Some(dir) = crate::local_state::screenshots_dir() else {
        return reply(false, "no benilla-config folder");
    };
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!("console-{stamp}.png"));
    world
        .commands()
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
    world.flush();
    reply(true, path.to_string_lossy())
}

fn state(world: &mut World) -> String {
    let client = world
        .get_resource::<State<crate::client_state::ClientState>>()
        .map(|s| format!("{:?}", s.get()));
    let mut out = format!("client={}", client.unwrap_or_else(|| "?".into()));
    if let Some(time) = world.get_resource::<Time<Real>>() {
        let dt = time.delta_secs();
        if dt > 0.0 {
            out += &format!(" frame_ms={:.1}", dt * 1000.0);
        }
    }
    if let Some(t) = crate::mobile::power::thermal_state() {
        out += &format!(" thermal={t}");
    }
    reply(true, out)
}

fn size(world: &mut World) -> String {
    match world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
    {
        Ok(w) => reply(
            true,
            format!("{} {} scale {}", w.width(), w.height(), w.scale_factor()),
        ),
        Err(_) => reply(false, "no window"),
    }
}

/// Lua commands, with the VM and the game state it reads lent to it.
fn serve_lua(mut queue: ResMut<LuaQueue>, mut vm: crate::script_calls::HostedVm) {
    if queue.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut queue.0);
    vm.run(|script| {
        for req in requests {
            let answer = match (script.as_deref(), req.line.as_str()) {
                (None, _) => reply(false, "no interface loaded"),
                (Some(s), "focus") => reply(true, s.focused_editbox_name().unwrap_or_default()),
                (Some(s), line) => run_lua(s, line.trim_start_matches("lua").trim()),
            };
            let _ = req.reply.send(answer);
        }
    });
}

fn run_lua(script: &benilla_ui::script::UiScript, code: &str) -> String {
    let chunk = match code.strip_prefix('=') {
        Some(expr) => format!("return {expr}"),
        None => code.to_string(),
    };
    match script.eval::<benilla_ui::mlua::Variadic<benilla_ui::mlua::Value>>(&chunk) {
        Ok(values) => reply(true, values.iter().map(show).collect::<Vec<_>>().join("\t")),
        Err(e) => reply(false, e.to_string()),
    }
}

fn show(v: &benilla_ui::mlua::Value) -> String {
    match v {
        benilla_ui::mlua::Value::Nil => "nil".into(),
        benilla_ui::mlua::Value::Boolean(b) => b.to_string(),
        benilla_ui::mlua::Value::Integer(i) => i.to_string(),
        benilla_ui::mlua::Value::Number(n) => n.to_string(),
        benilla_ui::mlua::Value::String(s) => s.to_string_lossy().to_string(),
        other => format!("<{}>", other.type_name()),
    }
}
