//! The mobile client's assists: the context button's object, the screen's safe area, and the
//! interface's ways back into the host (a haptic, the context button's tap). The interface half
//! is `assets/mobile/MobileAssist.xml`; its settings are the `mobile*` rows of the options
//! window's Assists page.
//!
//! The context button offers what the cursor would over the nearest object the player can act on
//! from where they stand: [`scan_context`] classifies each nearby unit, corpse and GameObject as
//! a hover over it would be ([`crate::target::cursor_mode::classify`]), keeps the nearest whose
//! cursor is a verb within reach, and a tap runs that object's own dispatcher
//! ([`crate::target::Interact`]), as a right-click on it does.

use std::sync::atomic::{AtomicBool, Ordering};

use benilla_protocol::EntityKind;
use benilla_ui::mlua;
use benilla_ui::script::UiScript;
use bevy::prelude::*;

use crate::net::{Guid, NetEntity, SelfPlayer};
use crate::target::cursor_mode::{self, CursorKind};
use crate::target::{Hovered, HoveredObject};

/// Objects farther than this are not asked about: every verb's reach is shorter.
const SCAN_RANGE: f32 = 10.0;
/// Seconds between scans.
const SCAN_EVERY: f32 = 0.2;

/// The object the context button acts on, and what it would do.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub(crate) struct ContextTarget(pub(crate) Option<(Entity, CursorKind, bool)>);

/// A tap on the context button, from the interface, for [`use_context`].
static CONTEXT_USE: AtomicBool = AtomicBool::new(false);

/// The verb the button shows for a cursor, and whether the object is a corpse's loot.
fn verb(kind: CursorKind, dead: bool) -> Option<&'static str> {
    Some(match kind {
        CursorKind::Speak => "Talk",
        CursorKind::Pickup if dead => "Loot",
        CursorKind::Pickup => "Trade",
        CursorKind::LootAll => "Loot",
        CursorKind::Interact => "Use",
        CursorKind::Buy => "Buy",
        CursorKind::Inspect => "Read",
        CursorKind::Trainer => "Train",
        CursorKind::Taxi => "Fly",
        CursorKind::Skin => "Skin",
        CursorKind::Mail => "Mail",
        CursorKind::Mine => "Mine",
        CursorKind::GatherHerbs => "Gather",
        CursorKind::PickLock => "Pick Lock",
        CursorKind::Point
        | CursorKind::Attack
        | CursorKind::Repair
        | CursorKind::Cast
        | CursorKind::Custom => return None,
    })
}

/// Find the nearest object the player can act on, a few times a second.
#[allow(clippy::type_complexity)] // a Bevy system's full input set
pub(crate) fn scan_context(
    inputs: cursor_mode::CursorInputs,
    objects: Query<
        (
            Entity,
            &Guid,
            &NetEntity,
            &Transform,
            Option<&crate::net::ObjectStore>,
        ),
        Without<SelfPlayer>,
    >,
    me: Query<&Transform, With<SelfPlayer>>,
    time: Res<Time>,
    mut since: Local<f32>,
    mut target: ResMut<ContextTarget>,
) {
    *since += time.delta_secs();
    if *since < SCAN_EVERY {
        return;
    }
    *since = 0.0;
    let Ok(me) = me.single() else {
        target.set_if_neq(ContextTarget(None));
        return;
    };
    let mut best: Option<(f32, Entity, CursorKind, bool)> = None;
    for (entity, guid, net, tf, store) in &objects {
        let d2 = tf.translation.distance_squared(me.translation);
        if d2 > SCAN_RANGE * SCAN_RANGE || best.is_some_and(|(b, ..)| d2 >= b) {
            continue;
        }
        let (mut hovered, mut object) = (Hovered::default(), HoveredObject::default());
        let dead = match net.kind {
            EntityKind::Unit => {
                (hovered.target, hovered.guid) = (Some(entity), Some(guid.0));
                store.is_some_and(|s| s.0.unit_is_dead())
            }
            EntityKind::Corpse => {
                (hovered.corpse, hovered.corpse_guid) = (Some(entity), Some(guid.0));
                true
            }
            EntityKind::GameObject => {
                (object.target, object.guid) = (Some(entity), Some(guid.0));
                false
            }
            EntityKind::Player | EntityKind::DynamicObject | EntityKind::Other => continue,
        };
        let (cursor, _) = cursor_mode::classify(&inputs, &hovered, &object);
        if !cursor.unable && verb(cursor.kind, dead).is_some() {
            best = Some((d2, entity, cursor.kind, dead));
        }
    }
    target.set_if_neq(ContextTarget(best.map(|(_, e, k, dead)| (e, k, dead))));
}

/// The context button's tap: the object's own dispatcher, as a right-click on it runs.
pub(crate) fn use_context(
    target: Res<ContextTarget>,
    mut interact: MessageWriter<crate::target::Interact>,
) {
    if CONTEXT_USE.swap(false, Ordering::SeqCst) {
        if let Some((entity, ..)) = target.0 {
            interact.write(crate::target::Interact(entity));
        }
    }
}

/// The screen's safe area as shares of the window, left, right, top and bottom: where the
/// Dynamic Island, the rounded corners and the home indicator are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SafeArea(pub(crate) [f32; 4]);

/// What the interface has been told, per VM.
#[derive(Resource, Default)]
pub(crate) struct AssistLua {
    context: Option<Option<(Entity, CursorKind, bool)>>,
    safe: Option<SafeArea>,
    session: u64,
}

/// Tell the interface the context button's verb and the safe area, when either changes.
pub(crate) fn serve_assist_lua(
    target: Res<ContextTarget>,
    mut told: ResMut<AssistLua>,
    mut vm: crate::script_calls::HostedVm,
) {
    let safe = safe_area();
    let (context, seen) = (target.0, told.session);
    let mut stale = (told.context != Some(context), told.safe != Some(safe));
    let session = vm.run(|script| {
        let script = script?;
        if script.session() != seen {
            stale = (true, true);
        }
        if stale.0 {
            let line = match context.and_then(|(_, kind, dead)| Some((kind, verb(kind, dead)?))) {
                Some((kind, verb)) => format!(
                    "BENILLA_MOBILE_CONTEXT_VERB = \"{verb}\"; BENILLA_MOBILE_CONTEXT_ICON = \"Interface\\\\Cursor\\\\{}\"",
                    kind.name()
                ),
                None => "BENILLA_MOBILE_CONTEXT_VERB = nil; BENILLA_MOBILE_CONTEXT_ICON = nil".into(),
            };
            let _ = script.run(&line);
            let _ = script.run("if BenillaMobileContext_Update then BenillaMobileContext_Update() end");
        }
        if stale.1 {
            let [l, r, t, b] = safe.0;
            let _ = script.run(&format!(
                "BENILLA_MOBILE_SAFE = {{ left = {l}, right = {r}, top = {t}, bottom = {b} }}; \
                 if BenillaMobileSafe_Apply then BenillaMobileSafe_Apply() end"
            ));
        }
        Some(script.session())
    });
    if let Some(session) = session {
        told.session = session;
        told.context = Some(context);
        told.safe = Some(safe);
    }
}

/// Put the host's functions in the world VM: `BenillaMobile_Haptic(style)` and
/// `BenillaMobile_ContextUse()`.
pub(crate) fn install(script: &UiScript) -> mlua::Result<()> {
    let lua = script.lua();
    let g = lua.globals();
    g.set(
        "BenillaMobile_Haptic",
        lua.create_function(|_, style: Option<String>| {
            haptic(style.as_deref().unwrap_or("light"));
            Ok(())
        })?,
    )?;
    g.set(
        "BenillaMobile_ContextUse",
        lua.create_function(|_, ()| {
            CONTEXT_USE.store(true, Ordering::SeqCst);
            Ok(())
        })?,
    )?;
    Ok(())
}

/// The haptics a test reads: every style asked for, in order.
#[cfg(test)]
pub(crate) static HAPTICS_PLAYED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// A haptic: `light`, `medium` or `heavy` for an impact, `success`, `warning` or `error` for a
/// notification. Off iOS it does nothing.
fn haptic(style: &str) {
    #[cfg(test)]
    HAPTICS_PLAYED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(style.to_string());
    #[cfg(target_os = "ios")]
    ios::haptic(style);
    #[cfg(not(any(target_os = "ios", test)))]
    let _ = style;
}

fn safe_area() -> SafeArea {
    #[cfg(target_os = "ios")]
    {
        ios::safe_area().unwrap_or_default()
    }
    #[cfg(not(target_os = "ios"))]
    {
        // `$BENILLA_SAFE_AREA=l,r,t,b` (shares of the window) tries the layout on a desktop.
        std::env::var("BENILLA_SAFE_AREA")
            .ok()
            .and_then(|v| {
                let n: Vec<f32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
                (n.len() == 4).then(|| SafeArea([n[0], n[1], n[2], n[3]]))
            })
            .unwrap_or_default()
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_ui_kit::{
        UIApplication, UIImpactFeedbackGenerator, UIImpactFeedbackStyle,
        UINotificationFeedbackGenerator, UINotificationFeedbackType,
    };

    thread_local! {
        static IMPACTS: std::cell::RefCell<Vec<(isize, Retained<UIImpactFeedbackGenerator>)>> =
            const { std::cell::RefCell::new(Vec::new()) };
        static NOTIFY: std::cell::RefCell<Option<Retained<UINotificationFeedbackGenerator>>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(super) fn haptic(style: &str) {
        // The VM runs on the main thread, where UIKit lives; anywhere else, nothing.
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let impact = match style {
            "light" => Some(UIImpactFeedbackStyle::Light),
            "medium" => Some(UIImpactFeedbackStyle::Medium),
            "heavy" => Some(UIImpactFeedbackStyle::Heavy),
            _ => None,
        };
        if let Some(kind) = impact {
            IMPACTS.with(|cell| {
                let mut gens = cell.borrow_mut();
                let generator = match gens.iter().find(|(k, _)| *k == kind.0) {
                    Some((_, g)) => g.clone(),
                    None => {
                        // The view-less initializer: the haptic is the device's, not a view's.
                        #[allow(deprecated)]
                        let g = UIImpactFeedbackGenerator::initWithStyle(
                            UIImpactFeedbackGenerator::alloc(mtm),
                            kind,
                        );
                        gens.push((kind.0, g.clone()));
                        g
                    }
                };
                generator.impactOccurred();
            });
            return;
        }
        let kind = match style {
            "success" => UINotificationFeedbackType::Success,
            "error" => UINotificationFeedbackType::Error,
            _ => UINotificationFeedbackType::Warning,
        };
        NOTIFY.with(|cell| {
            let generator = cell
                .borrow_mut()
                .get_or_insert_with(|| UINotificationFeedbackGenerator::new(mtm))
                .clone();
            generator.notificationOccurred(kind);
        });
    }

    /// The key window's safe-area insets as shares of its size.
    #[allow(deprecated)] // `keyWindow`: the app has one scene and one window
    pub(super) fn safe_area() -> Option<super::SafeArea> {
        let mtm = MainThreadMarker::new()?;
        let window = UIApplication::sharedApplication(mtm).keyWindow()?;
        let insets = window.safeAreaInsets();
        let size = window.bounds().size;
        if size.width <= 0.0 || size.height <= 0.0 {
            return None;
        }
        Some(super::SafeArea([
            (insets.left / size.width) as f32,
            (insets.right / size.width) as f32,
            (insets.top / size.height) as f32,
            (insets.bottom / size.height) as f32,
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verbs_show_on_the_button() {
        assert_eq!(verb(CursorKind::Speak, false), Some("Talk"));
        assert_eq!(verb(CursorKind::Pickup, true), Some("Loot"));
        assert_eq!(verb(CursorKind::Pickup, false), Some("Trade"));
        assert_eq!(verb(CursorKind::GatherHerbs, false), Some("Gather"));
        assert_eq!(verb(CursorKind::Attack, false), None);
        assert_eq!(verb(CursorKind::Point, false), None);
    }
}
