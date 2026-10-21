# benilla on iOS

The iOS port: the same client, built for iPhone and iPad, with touch controls, the on-screen
keyboard, and the game data in the app's Documents folder.

## Controls

In the world:

| Touch | Does |
|---|---|
| Finger down on the left ~40% of the screen, off any window | Floating joystick: W/S forward and back, Q/E strafe |
| Tap | Left click (select, press a button) |
| Hold still ½ s | Right click (talk, loot, attack, use an item) |
| Drag over the world | Turn the camera and your character (right-button mouse-look) |
| Drag over a window | Left drag (move spells and items) |
| Two-finger pinch | Zoom (mouse wheel); scrolls a window under the fingers |
| Right-edge buttons, top to bottom | Menu (Esc), Target nearest (Tab), Chat (Enter), Jump (Space) |

Any text box: the keyboard opens when it has focus, Return is Enter, and a tap outside the box puts
the keyboard away (tap the box to bring it back). The last account that reached character select
signs in by itself on the next launch; a wrong password forgets it.

## Building and installing

Needs Xcode, the Rust iOS targets (`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`)
and XcodeGen (`brew install xcodegen`).

```sh
ios/run-sim.sh                    # build, install and launch in the Simulator (reads ~/wow/Data)
TEAM=XXXXXXXXXX ios/run-device.sh # build, sign with your team, install and launch on the iPhone
ios/run-device.sh --copy-data     # also copy ~/wow/Data into the app (first install)
ios/run-device.sh --no-build      # re-sign and reinstall the last build (e.g. after 7 days)
```

The first device build needs the project opened in Xcode once (`open ios/benilla.xcodeproj`, then
pick your team under Signing & Capabilities) so Xcode makes the provisioning profile; after that
the script signs on its own. A free Apple account's profile lasts 7 days: when the app stops
opening, run `ios/run-device.sh --no-build` from Terminal (if it reports "No Accounts", open the
project in Xcode once and build there).

The app's Documents folder holds `Data/` (the 1.12.1 install) and `benilla-config/` (settings,
addons). Both show in Finder (iPhone > Files > benilla) and the Files app.

## When something goes wrong

```sh
ios/device-logs.sh                # list benilla crash and memory reports on the iPhone
ios/device-logs.sh <report.ips>   # copy one over and print its summary
```

`WOW_TOUCH_ALWAYS=1 ios/run-sim.sh` shows the touch controls outside the world, and
`RUST_LOG=info,benilla_app::touch=debug` logs what each gesture became.
