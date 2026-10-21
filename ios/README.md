# benilla on iOS

The iOS port: the same client, built for iPhone and iPad, with touch controls, game controllers,
a launcher for servers and accounts, and the game data in the app's Documents folder. Its
interface is `crates/benilla-app/assets/mobile` (loaded after benilla's layer when the client is the
mobile client: on iOS, or on a desktop under `BENILLA_MOBILE=1`); its Rust is
`crates/benilla-app/src/mobile` and `crates/benilla-app/src/touch`.

## Launcher

The login screen opens on the launcher: saved servers on the left (a name, the realmlist address,
and a realm address override for a server behind NAT), the accounts saved on the selected server
on the right. A tap on an account signs in; "Sign in automatically" signs in to the last account
at launch. Passwords are in the iOS Keychain. "Sign In Manually" shows the stock boxes; an account
that reaches character select there is saved on its server. A wrong password is forgotten.

## Touch

| Touch | Does |
|---|---|
| Thumb down on the left, off any window | Floating movement stick (W/S forward and back, Q/E strafe) |
| Tap | Left click |
| Hold still (Long Press Time) | Right click (talk, loot, attack, use an item) |
| Drag over the world | Turn the camera and your character |
| Drag over a window | Left drag (move spells and items) |
| Two fingers: spread / move together | Zoom / scroll |

The jump ring (bottom right) is Jump with eight action slots round it: real action buttons on the
bar "Jump Ring Bar" names; drag a spell on to a slot to change it. The Menu button opens a radial
menu of every window, the four views (Combat, Explore, Social, Minimal) and the layout editor,
which moves, resizes, hides and rebinds every control. The controls fade while no finger is down.
Text boxes raise the keyboard; Return is Enter; a tap outside a box puts the keyboard away.

## Controllers

Backbone One, Xbox, DualSense or any MFi pad. Left stick moves, right stick looks, LB/RB cycle
targets, LT/RT/both switch the face buttons and d-pad between four layers (32 bindings), Menu opens
the radial menu, View the game menu, L3 autoruns, R3 the virtual cursor, which also opens by itself
for loot, vendor, quest and similar windows (right stick moves it, A/B click). Remap on Options >
Touch & Controller > Controller Buttons. The touch controls step aside while a controller is
connected.

## Settings

Options > Touch & Controller: touch controls, view, opacity, look speed, long press time, stick
size and area, jump ring bar, controller support, look speed and invert, sign in automatically,
remote console. The UI scale reaches 160%; rows that mean nothing on a phone are greyed out.

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
launcher, addons). Both show in Finder (iPhone > Files > benilla) and the Files app.

## Debugging

```sh
ios/device-logs.sh                # list benilla crash and memory reports on the iPhone
ios/device-logs.sh <report.ips>   # copy one over and print its summary
ios/console.py --host <phone-ip> --token <token> 'state' 'lua =GetZoneText()' 'shot'
```

The remote console (Options > Touch & Controller > Remote Console, or `BENILLA_CONSOLE=1`)
listens on port 7979: `lua`, `tap`, `hold`, `drag`, `pinch`, `key`, `type`, `shot`, `console`,
`state`, `size`, `focus`, `log` (see `crates/benilla-app/src/mobile/console.rs`). Its token is
`benilla-config/console-token`, or `BENILLA_CONSOLE_TOKEN`.

In the Simulator: `WOW_TOUCH_ALWAYS=1` shows the stick outside the world, `BENILLA_VIRTUAL_PAD=1`
connects Apple's on-screen controller, and `RUST_LOG=info,benilla_app::touch=debug` logs what each
gesture became. On a Mac, `BENILLA_MOBILE=1` loads the mobile interface, and
`WOW_CAPTURE=ui-cooldown WOW_CAPTURE_UI=1 WOW_WIN=1912x880` renders it in the world offline.
