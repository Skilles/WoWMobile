//! benilla: the first finger down drives a mouse.
//!
//! An app written for a mouse (cursor position, left button) sees touches as nothing at all. Beside
//! every `WindowEvent::Touch`, the touch that went down while no other was down (the primary) also
//! moves the cursor and holds the left button: down presses at the finger, a drag moves the cursor
//! with the button held, lifting releases. Other fingers only send their `Touch`. The cursor stays
//! where the finger lifted, as a mouse's does. `WINIT_IOS_TOUCH_AS_MOUSE=0` turns it off.

use std::cell::Cell;

use super::DEVICE_ID;
use crate::dpi::PhysicalPosition;
use crate::event::{ElementState, MouseButton, TouchPhase, WindowEvent};

thread_local! {
    static PRIMARY: Cell<Option<u64>> = const { Cell::new(None) };
    static ENABLED: bool = std::env::var_os("WINIT_IOS_TOUCH_AS_MOUSE").is_none_or(|v| v != "0");
}

/// The mouse events touch `id` in `phase` at `position` stands for, in order.
pub(crate) fn emulate(
    id: u64,
    phase: TouchPhase,
    position: PhysicalPosition<f64>,
) -> Vec<WindowEvent> {
    if !ENABLED.with(|e| *e) {
        return Vec::new();
    }
    let moved = WindowEvent::CursorMoved { device_id: DEVICE_ID, position };
    let button = |state| WindowEvent::MouseInput {
        device_id: DEVICE_ID,
        state,
        button: MouseButton::Left,
    };
    PRIMARY.with(|primary| match phase {
        TouchPhase::Started => {
            if primary.get().is_some() {
                return Vec::new();
            }
            primary.set(Some(id));
            vec![moved, button(ElementState::Pressed)]
        },
        TouchPhase::Moved => {
            if primary.get() != Some(id) {
                return Vec::new();
            }
            vec![moved]
        },
        TouchPhase::Ended | TouchPhase::Cancelled => {
            if primary.get() != Some(id) {
                return Vec::new();
            }
            primary.set(None);
            vec![moved, button(ElementState::Released)]
        },
    })
}
