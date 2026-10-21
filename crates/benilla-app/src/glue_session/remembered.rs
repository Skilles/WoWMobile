//! iOS: the last account that reached character select, kept in the Keychain, signs in by itself
//! once per launch, as `$WOW_USER`/`$WOW_PASS` do on desktop. A refusal of the credentials
//! (realmd's `0x04`/`0x05`) forgets them; a logout back to the login screen does not sign in
//! again until the next launch.
//!
//! Deviation: 1.12.1 remembers only the account name (`accountName`, "Remember Account Name");
//! typing a password on a phone keyboard every launch is the reason.

use bevy::prelude::*;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use super::login::LoginIntent;
use crate::net::CharListMessage;

/// The Keychain item: one generic password holding `account\npassword`.
const SERVICE: &str = "dev.benilla.login";
const ITEM: &str = "last";

/// The remembered account and password, if any.
pub(crate) fn load() -> Option<(String, String)> {
    let bytes = get_generic_password(SERVICE, ITEM).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let (user, pass) = text.split_once('\n')?;
    (!user.is_empty() && !pass.is_empty()).then(|| (user.to_string(), pass.to_string()))
}

/// Forget the remembered account (its credentials were refused).
pub(crate) fn forget() {
    if delete_generic_password(SERVICE, ITEM).is_ok() {
        info!("login: forgot the remembered account");
    }
}

/// Whether realmd's refusal says the account or password is wrong (`WOW_FAIL_UNKNOWN_ACCOUNT`,
/// which vmangos and CMaNGOS send for a bad password too, and `WOW_FAIL_INCORRECT_PASSWORD`).
pub(crate) fn refuses_credentials(refusal: benilla_protocol::LoginRefusal) -> bool {
    use benilla_protocol::LoginRefusal::{Logon, LogonProof};
    matches!(refusal, Logon(0x04 | 0x05) | LogonProof(0x04 | 0x05))
}

/// The roster arrived, so the session's credentials are good: remember them.
pub(crate) fn remember_on_roster(
    mut rosters: MessageReader<CharListMessage>,
    intent: Res<LoginIntent>,
) {
    if rosters.read().last().is_none() {
        return;
    }
    let Some((user, pass)) = intent.creds() else {
        return;
    };
    if load().is_some_and(|(u, p)| &u == user && &p == pass) {
        return;
    }
    match set_generic_password(SERVICE, ITEM, format!("{user}\n{pass}").as_bytes()) {
        Ok(()) => info!("login: remembering {user} for the next launch"),
        Err(e) => warn!("login: could not remember {user} in the Keychain: {e}"),
    }
}
