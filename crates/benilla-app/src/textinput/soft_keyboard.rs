//! The on-screen keyboard (iOS): shown while a visible EditBox holds keyboard focus, hidden
//! otherwise. bevy's `Window::ime_enabled` becomes winit's `set_ime_allowed`, which on iOS makes the
//! view first responder and so raises the keyboard. Its keys come back through winit as
//! `KeyboardInput` (`insertText:` and `deleteBackward`), the same input the EditBoxes read on desktop.
//!
//! `Window::ime_position` (winit's `set_ime_cursor_area`) carries the focused box's bottom-left
//! corner; benilla's winit lifts the window by as much as the keyboard would cover it.
//!
//! As in any iOS app, a tap outside the focused box puts the keyboard away (the box keeps its focus,
//! as a click elsewhere leaves it on desktop), and a tap on the box, or focus moving to another,
//! brings it back. That is how a box the keyboard covers, the login screen's password, is reached.

use benilla_ui::layout::Rect;
use benilla_ui::script::UiScript;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// The keyboard was put away while this box (its rect) held focus.
#[derive(Default)]
pub(crate) struct Dismissed(Option<Rect>);

/// Raise or drop the keyboard as EditBox focus changes, and keep the box's position current.
pub(crate) fn follow_editbox_focus(
    script: Option<NonSend<UiScript>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut dismissed: Local<crate::ui_script::VmMemo<Dismissed>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let rect = script.as_ref().and_then(|s| s.focused_editbox_rect());
    let dismissed = dismissed.get_for(script.as_deref());
    let s = crate::ui_script::seam_scale(window.height());
    if rect.is_none() || dismissed.0.is_some_and(|d| Some(d) != rect) {
        dismissed.0 = None;
    }
    if let (Some(r), true) = (rect, buttons.just_pressed(MouseButton::Left)) {
        if let Some(cursor) = window.cursor_position() {
            let p = crate::ui_script::window_to_ui(window.height(), cursor);
            let inside = p.x >= r.left && p.x <= r.right && p.y >= r.bottom && p.y <= r.top;
            if inside {
                dismissed.0 = None;
            } else {
                dismissed.0 = Some(r);
            }
        }
    }
    let want = rect.is_some() && dismissed.0.is_none();
    if window.ime_enabled != want {
        window.ime_enabled = want;
    }
    if let Some(r) = rect {
        // Root units (768 high, y up) to the window's logical pixels (y down).
        let bottom_left = Vec2::new(r.left * s, window.height() - r.bottom * s);
        if window.ime_position != bottom_left {
            window.ime_position = bottom_left;
        }
    }
}
