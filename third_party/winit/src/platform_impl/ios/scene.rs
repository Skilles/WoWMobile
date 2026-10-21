//! benilla: UIScene lifecycle adoption.
//!
//! iOS 27 traps at launch (`_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`) in an
//! app built against its SDK that does not declare `UIApplicationSceneManifest`. Under a scene
//! manifest a `UIWindow` made with `initWithFrame:` is never attached to a scene and never shows, so
//! when the app declares one, launch waits for the scene to connect
//! (`UISceneWillConnectNotification`) instead of `UIApplicationDidFinishLaunchingNotification`, and
//! windows are made with `initWithWindowScene:` on that scene. Without a manifest nothing changes.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send_id};
use objc2_foundation::{MainThreadMarker, NSString};

thread_local! {
    static WINDOW_SCENE: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static LAUNCHED: Cell<bool> = const { Cell::new(false) };
}

/// The notification UIKit posts when a scene connects (`UISceneWillConnectNotification`).
pub(crate) fn will_connect_notification() -> Retained<NSString> {
    NSString::from_str("UISceneWillConnectNotification")
}

/// Whether the bundle's Info.plist declares `UIApplicationSceneManifest`.
pub(crate) fn uses_scenes(_mtm: MainThreadMarker) -> bool {
    let key = NSString::from_str("UIApplicationSceneManifest");
    unsafe {
        let bundle: Retained<AnyObject> = msg_send_id![class!(NSBundle), mainBundle];
        let manifest: Option<Retained<AnyObject>> =
            msg_send_id![&bundle, objectForInfoDictionaryKey: &*key];
        manifest.is_some()
    }
}

/// Remember the connected window scene (the notification's `object`). Returns `true` the first
/// time, when launch should proceed.
pub(crate) fn connect(_mtm: MainThreadMarker, scene: Option<Retained<AnyObject>>) -> bool {
    if let Some(scene) = scene {
        WINDOW_SCENE.with(|s| {
            if s.borrow().is_none() {
                *s.borrow_mut() = Some(scene);
            }
        });
    }
    !LAUNCHED.with(|l| l.replace(true))
}

/// The connected window scene, if the app runs under a scene manifest and one has connected.
pub(crate) fn window_scene(_mtm: MainThreadMarker) -> Option<Retained<AnyObject>> {
    WINDOW_SCENE.with(|s| s.borrow().clone())
}
