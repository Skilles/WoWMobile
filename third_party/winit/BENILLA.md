# benilla's `winit` fork: what differs from upstream

Upstream: [`rust-windowing/winit`](https://github.com/rust-windowing/winit), Apache-2.0, version
`0.30.13`, the version Bevy 0.18 resolves to. Wired in through `[patch.crates-io]` in the
workspace root. Copied from the cargo registry as published, minus `examples/`, `tests/` and the
lock file.

## Why

iOS 27 traps at launch (`_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`, SIGTRAP)
in any app built against its SDK that has not adopted the UIScene lifecycle. winit 0.30 (and
upstream `master` as of 2026-10) drives iOS only through `UIApplication` notifications and makes
its `UIWindow` with `initWithFrame:`, which under a scene manifest is never attached to a scene.

## What changed (iOS only)

- `src/platform_impl/ios/scene.rs` (new): reads whether Info.plist declares
  `UIApplicationSceneManifest`, and keeps the connected `UIWindowScene`.
- `event_loop.rs`: with a manifest, `did_finish_launching` runs on the first
  `UISceneWillConnectNotification` instead of `UIApplicationDidFinishLaunchingNotification`.
- `window.rs`: `WinitUIWindow::new` uses `initWithWindowScene:` when a scene has connected.

Without a scene manifest the iOS backend behaves as upstream. Every change is marked `benilla:`.
- `src/platform_impl/ios/keyboard.rs` (new): `set_ime_cursor_area` (ignored upstream) is honoured
  by sliding the `UIWindow` up while the on-screen keyboard would cover the area, from
  `UIKeyboardWillChangeFrameNotification` / `UIKeyboardWillHideNotification` observed in
  `event_loop.rs`.
- `event_loop.rs` / `app_state.rs`: `ActiveEventLoop::exit` (ignored upstream, per Apple's QA1561)
  runs the terminate path (`Destroyed`, `LoopExiting`) at the next loop turn and then ends the
  process, so a game's own Quit works.
- `window.rs`: `set_cursor_position` and `set_cursor_grab` succeed as no-ops instead of
  `NotSupported`; the app's own touch layer drives its cursor.
- `view.rs`: the on-screen keyboard's Return (`insertText:` with "\n") is sent as the Enter key
  (`NamedKey::Enter`, `KeyCode::Enter`), not as a "\n" character.
- `app_state.rs`: `did_finish_launching` disables the idle timer, so the screen does not dim and
  lock during play.
- `window.rs`: a window made on a scene skips `setScreen:` for borderless fullscreen (a UIKit error
  under the scene lifecycle; the scene sets the screen).
