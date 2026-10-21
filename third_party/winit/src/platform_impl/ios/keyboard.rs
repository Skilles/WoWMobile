//! benilla: keep the IME cursor area above the on-screen keyboard.
//!
//! Upstream ignores `set_ime_cursor_area` on iOS, so a text field in the lower half of a full-screen
//! app ends up under the keyboard. Here the area (logical points, window space) and the keyboard's
//! top edge (`UIKeyboardWillChangeFrameNotification`) are tracked, and the `UIWindow` slides up by
//! however much the keyboard would cover the area's bottom, and back down when it hides. Touches
//! read `locationInView(None)`, window space, so they stay matched to what is drawn.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, msg_send_id, ClassType};
use objc2_foundation::{CGFloat, CGPoint, CGRect, MainThreadMarker, NSNotification, NSString};
use objc2_ui_kit::UIWindow;

/// Clear space kept between the area's bottom and the keyboard's top, in points.
const MARGIN: CGFloat = 8.0;

thread_local! {
    static WINDOW: RefCell<Option<Retained<UIWindow>>> = const { RefCell::new(None) };
    /// The IME cursor area's bottom edge, in the unshifted window's points.
    static AREA_BOTTOM: Cell<Option<CGFloat>> = const { Cell::new(None) };
    /// The keyboard's top edge in screen points while it is shown.
    static KEYBOARD_TOP: Cell<Option<CGFloat>> = const { Cell::new(None) };
}

pub(crate) const WILL_CHANGE_FRAME: &str = "UIKeyboardWillChangeFrameNotification";
pub(crate) const WILL_HIDE: &str = "UIKeyboardWillHideNotification";

/// The window the keyboard belongs to; set when the app allows IME on it.
pub(crate) fn track_window(_mtm: MainThreadMarker, window: &UIWindow) {
    WINDOW.with(|w| *w.borrow_mut() = Some(window.retain()));
}

/// `set_ime_cursor_area` with the area's bottom edge in logical points.
pub(crate) fn set_area_bottom(mtm: MainThreadMarker, bottom: CGFloat) {
    AREA_BOTTOM.with(|a| a.set(Some(bottom)));
    relayout(mtm);
}

/// The keyboard is about to move or resize.
pub(crate) fn keyboard_will_change(mtm: MainThreadMarker, notification: &NSNotification) {
    let top = unsafe {
        let info: Option<Retained<AnyObject>> = msg_send_id![notification, userInfo];
        let key = NSString::from_str("UIKeyboardFrameEndUserInfoKey");
        info.and_then(|info| {
            let value: Option<Retained<AnyObject>> = msg_send_id![&info, objectForKey: &*key];
            value.map(|value| {
                let rect: CGRect = msg_send![&value, CGRectValue];
                rect.origin.y
            })
        })
    };
    KEYBOARD_TOP.with(|k| k.set(top));
    relayout(mtm);
}

/// The keyboard is about to go away.
pub(crate) fn keyboard_will_hide(mtm: MainThreadMarker) {
    KEYBOARD_TOP.with(|k| k.set(None));
    relayout(mtm);
}

fn relayout(_mtm: MainThreadMarker) {
    let Some(window) = WINDOW.with(|w| w.borrow().clone()) else {
        return;
    };
    let shift = match (AREA_BOTTOM.with(Cell::get), KEYBOARD_TOP.with(Cell::get)) {
        (Some(bottom), Some(top)) => (bottom + MARGIN - top).max(0.0),
        _ => 0.0,
    };
    let mut frame = window.frame();
    let origin = CGPoint { x: frame.origin.x, y: -shift };
    if frame.origin.y != origin.y {
        frame.origin = origin;
        window.setFrame(frame);
    }
}
