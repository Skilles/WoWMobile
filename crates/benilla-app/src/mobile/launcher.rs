//! The launcher's store: server profiles (a name, the realmlist address, and a realm address that
//! overrides the one realmd hands out, for a server behind NAT) and the accounts saved on each,
//! their passwords kept apart in [`secrets`]. The glue interface (`MobileLauncher.xml`) reads and
//! edits it through the `BenillaLauncher_*` functions [`install`] puts in the glue VM, and signs in
//! through the stock `DefaultServerLogin`. An account that reaches character select is saved on
//! the profile it dialed ([`remember_on_roster`]), and realmd refusing its password forgets the
//! password ([`forget_refused`]).
//!
//! Deviation: 1.12.1 remembers one account name (`accountName`) and no passwords; on a phone,
//! typing them at every launch is the reason.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use benilla_ui::mlua;
use benilla_ui::script::UiScript;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The CVar that overrides realmd's realm address for the next world dial (`""`: none).
pub(crate) const CVAR_REALM_OVERRIDE: &str = "mobileRealmOverride";

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct Profile {
    pub(crate) name: String,
    /// The realmlist, `host[:port]`.
    pub(crate) address: String,
    /// The world server's address, `host[:port]`, in place of realmd's; empty for realmd's own.
    #[serde(default)]
    pub(crate) realm_override: String,
    #[serde(default)]
    pub(crate) accounts: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct Store {
    #[serde(default)]
    pub(crate) profiles: Vec<Profile>,
    /// The profile and account last signed in, 0-based.
    #[serde(default)]
    pub(crate) last: Option<(usize, usize)>,
}

impl Store {
    /// The profile that dials `address`, made under that name if there is none.
    pub(crate) fn profile_for(&mut self, address: &str) -> usize {
        if let Some(i) = self
            .profiles
            .iter()
            .position(|p| p.address.eq_ignore_ascii_case(address))
        {
            return i;
        }
        self.profiles.push(Profile {
            name: address.to_string(),
            address: address.to_string(),
            ..Default::default()
        });
        self.profiles.len() - 1
    }

    /// Account `name` on profile `i`, added if it is new; account names are case-blind.
    pub(crate) fn account_on(&mut self, i: usize, name: &str) -> Option<usize> {
        let p = self.profiles.get_mut(i)?;
        if let Some(j) = p.accounts.iter().position(|a| a.eq_ignore_ascii_case(name)) {
            return Some(j);
        }
        p.accounts.push(name.to_string());
        Some(p.accounts.len() - 1)
    }
}

/// The secret's key: the realmlist and the account, as realmd matches them.
pub(crate) fn secret_key(address: &str, account: &str) -> String {
    format!(
        "{}|{}",
        address.to_ascii_lowercase(),
        account.to_ascii_uppercase()
    )
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
/// `mobileAutoLogin`, mirrored for the glue, which has no `GetCVar` ([`on_cvar`]).
static AUTO_LOGIN: AtomicBool = AtomicBool::new(true);
static AUTO_LOGIN_READ: std::sync::Once = std::sync::Once::new();

fn auto_login() -> bool {
    AUTO_LOGIN_READ.call_once(|| {
        if let Some(v) = crate::cvars::boot_cvar("mobileAutoLogin") {
            AUTO_LOGIN.store(benilla_ui::script::s_str_to_int(&v) != 0, Ordering::SeqCst);
        }
    });
    AUTO_LOGIN.load(Ordering::SeqCst)
}

/// Keeps [`AUTO_LOGIN`] with the CVar.
pub(crate) fn on_cvar(ev: On<crate::cvars::CvarChanged>) {
    if ev.is("mobileAutoLogin") {
        AUTO_LOGIN_READ.call_once(|| {});
        AUTO_LOGIN.store(ev.flag(), Ordering::SeqCst);
    }
}
/// The automatic sign-in is offered once a launch.
static AUTO_TAKEN: AtomicBool = AtomicBool::new(false);

fn path() -> Option<std::path::PathBuf> {
    Some(crate::local_state::home()?.join("launcher.json"))
}

/// Run `f` on the store, loaded on first use; a change is written back.
pub(crate) fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let store = guard.get_or_insert_with(load);
    let before = store.clone();
    let out = f(store);
    if *store != before {
        save(store);
    }
    out
}

fn load() -> Store {
    let mut store: Store = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    // The earlier sign-in leaves the Keychain as it moves, so the store it lands in is written
    // now: `with_store` compares against the store after the load.
    let before = store.clone();
    migrate_single_login(&mut store);
    if store != before {
        save(&store);
    }
    store
}

fn save(store: &Store) {
    let Some(path) = path() else {
        return;
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
    match serde_json::to_string_pretty(store) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                warn!("launcher: could not save {}: {e}", path.display());
            }
        }
        Err(e) => warn!("launcher: {e}"),
    }
}

/// The one login the first iOS builds kept (`dev.benilla.login`/`last`), made a profile on the
/// realmlist it was used with.
fn migrate_single_login(store: &mut Store) {
    let Some((user, pass)) = secrets::take_legacy() else {
        return;
    };
    let address = crate::cvars::boot_cvar(crate::realmlist::CVAR_REALMLIST)
        .and_then(|a| crate::realmlist::normalize(&a))
        .unwrap_or_else(|| "localhost".to_string());
    let i = store.profile_for(&address);
    if let Some(j) = store.account_on(i, &user) {
        secrets::set(&secret_key(&address, &user), &pass);
        store.last = Some((i, j));
        info!("launcher: kept {user} on {address} from the earlier sign-in");
    }
}

/// Put the `BenillaLauncher_*` functions in the glue VM.
pub(crate) fn install(script: &UiScript) -> mlua::Result<()> {
    let lua = script.lua();
    let g = lua.globals();
    // Lua's indices are 1-based; a bad one answers nil.
    fn ix(i: Option<i64>) -> Option<usize> {
        i.filter(|i| *i >= 1).map(|i| (i - 1) as usize)
    }

    g.set(
        "BenillaLauncher_NumProfiles",
        lua.create_function(|_, ()| Ok(with_store(|s| s.profiles.len() as i64)))?,
    )?;
    g.set(
        "BenillaLauncher_GetProfile",
        lua.create_function(|_, i: Option<i64>| {
            Ok(with_store(|s| {
                ix(i).and_then(|i| s.profiles.get(i)).map(|p| {
                    (
                        p.name.clone(),
                        p.address.clone(),
                        p.realm_override.clone(),
                        p.accounts.len() as i64,
                    )
                })
            })
            .map_or((None, None, None, None), |(a, b, c, d)| {
                (Some(a), Some(b), Some(c), Some(d))
            }))
        })?,
    )?;
    g.set(
        "BenillaLauncher_GetAccount",
        lua.create_function(|_, (i, j): (Option<i64>, Option<i64>)| {
            Ok(with_store(|s| {
                let p = ix(i).and_then(|i| s.profiles.get(i))?;
                let name = ix(j).and_then(|j| p.accounts.get(j))?.clone();
                let saved = secrets::get(&secret_key(&p.address, &name)).is_some();
                Some((name, saved.then_some(1)))
            })
            .map_or((None, None), |(n, s)| (Some(n), s)))
        })?,
    )?;
    g.set(
        "BenillaLauncher_GetLast",
        lua.create_function(|_, ()| {
            Ok(with_store(|s| s.last).map_or((None, None), |(i, j)| {
                (Some(i as i64 + 1), Some(j as i64 + 1))
            }))
        })?,
    )?;
    g.set(
        "BenillaLauncher_SaveProfile",
        lua.create_function(
            |_, (i, name, address, over): (Option<i64>, String, String, Option<String>)| {
                let Some(address) = crate::realmlist::normalize(&address) else {
                    return Ok((None, Some("That is not a server address.".to_string())));
                };
                let over = over.unwrap_or_default();
                let over = if over.trim().is_empty() {
                    String::new()
                } else if let Some(o) = crate::realmlist::normalize(&over) {
                    o
                } else {
                    return Ok((None, Some("That is not a realm address.".to_string())));
                };
                let name = if name.trim().is_empty() {
                    address.clone()
                } else {
                    name.trim().to_string()
                };
                Ok(with_store(|s| {
                    let at = ix(i).filter(|i| *i < s.profiles.len());
                    let profile = Profile {
                        name,
                        address,
                        realm_override: over,
                        accounts: at
                            .map(|i| s.profiles[i].accounts.clone())
                            .unwrap_or_default(),
                    };
                    let index = match at {
                        Some(i) => {
                            s.profiles[i] = profile;
                            i
                        }
                        None => {
                            s.profiles.push(profile);
                            s.profiles.len() - 1
                        }
                    };
                    (Some(index as i64 + 1), None)
                }))
            },
        )?,
    )?;
    g.set(
        "BenillaLauncher_DeleteProfile",
        lua.create_function(|_, i: Option<i64>| {
            with_store(|s| {
                if let Some(i) = ix(i).filter(|i| *i < s.profiles.len()) {
                    let p = s.profiles.remove(i);
                    for a in &p.accounts {
                        secrets::delete(&secret_key(&p.address, a));
                    }
                    s.last = None;
                }
            });
            Ok(())
        })?,
    )?;
    g.set(
        "BenillaLauncher_SaveAccount",
        lua.create_function(
            |_, (i, name, password): (Option<i64>, String, Option<String>)| {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Ok(None);
                }
                Ok(with_store(|s| {
                    let i = ix(i).filter(|i| *i < s.profiles.len())?;
                    let j = s.account_on(i, &name)?;
                    if let Some(pass) = password.filter(|p| !p.is_empty()) {
                        secrets::set(&secret_key(&s.profiles[i].address, &name), &pass);
                    }
                    Some(j as i64 + 1)
                }))
            },
        )?,
    )?;
    g.set(
        "BenillaLauncher_DeleteAccount",
        lua.create_function(|_, (i, j): (Option<i64>, Option<i64>)| {
            with_store(|s| {
                let Some(i) = ix(i).filter(|i| *i < s.profiles.len()) else {
                    return;
                };
                let p = &mut s.profiles[i];
                if let Some(j) = ix(j).filter(|j| *j < p.accounts.len()) {
                    let name = p.accounts.remove(j);
                    secrets::delete(&secret_key(&p.address, &name));
                    s.last = None;
                }
            });
            Ok(())
        })?,
    )?;
    // What a sign-in needs: the account, its saved password (nil if none), the realmlist and the
    // realm override. Marks the pair the last used.
    g.set(
        "BenillaLauncher_Credentials",
        lua.create_function(|_, (i, j): (Option<i64>, Option<i64>)| {
            Ok(with_store(|s| {
                let (i, j) = (ix(i)?, ix(j)?);
                let p = s.profiles.get(i)?;
                let name = p.accounts.get(j)?.clone();
                let pass = secrets::get(&secret_key(&p.address, &name));
                let out = (name, pass, p.address.clone(), p.realm_override.clone());
                s.last = Some((i, j));
                Some(out)
            })
            .map_or((None, None, None, None), |(a, b, c, d)| {
                (Some(a), b, Some(c), Some(d))
            }))
        })?,
    )?;
    g.set(
        "BenillaLauncher_AutoLoginEnabled",
        lua.create_function(|_, ()| Ok(auto_login().then_some(1)))?,
    )?;
    // The glue writes the CVar through ConsoleExec, which lands later; the mirror moves now.
    g.set(
        "BenillaLauncher_SetAutoLogin",
        lua.create_function(|_, on: Option<bool>| {
            AUTO_LOGIN_READ.call_once(|| {});
            AUTO_LOGIN.store(on.unwrap_or(false), Ordering::SeqCst);
            Ok(())
        })?,
    )?;
    // The pair to sign in to automatically, once a launch: the last one, if it has a password.
    g.set(
        "BenillaLauncher_TakeAutoLogin",
        lua.create_function(|_, ()| {
            if !auto_login() || AUTO_TAKEN.swap(true, Ordering::SeqCst) {
                return Ok((None, None));
            }
            Ok(with_store(|s| {
                let (i, j) = s.last?;
                let p = s.profiles.get(i)?;
                let name = p.accounts.get(j)?;
                secrets::get(&secret_key(&p.address, name))?;
                Some((i as i64 + 1, j as i64 + 1))
            })
            .map_or((None, None), |(i, j)| (Some(i), Some(j))))
        })?,
    )?;
    Ok(())
}

/// The roster arrived, so this session's account and password are good: save them on the profile
/// for the realmlist they dialed, and make it the last used.
pub(crate) fn remember_on_roster(
    mut rosters: MessageReader<crate::net::CharListMessage>,
    intent: Res<crate::glue_session::login::LoginIntent>,
    realmlist: Res<crate::realmlist::Realmlist>,
) {
    if rosters.read().last().is_none() {
        return;
    }
    let Some((user, pass)) = intent.creds() else {
        return;
    };
    let address = realmlist.address().to_string();
    with_store(|s| {
        let i = s.profile_for(&address);
        let Some(j) = s.account_on(i, user) else {
            return;
        };
        let key = secret_key(&address, user);
        if secrets::get(&key).as_deref() != Some(pass.as_str()) {
            secrets::set(&key, pass);
            info!("launcher: saved {user} on {address}");
        }
        s.last = Some((i, j));
    });
}

/// realmd refused `user`'s password on `address`: forget it, so the launcher asks again.
pub(crate) fn forget_refused(address: &str, user: &str) {
    if secrets::delete(&secret_key(address, user)) {
        info!("launcher: forgot the password for {user} on {address}");
    }
}

/// Where passwords are kept: the Keychain on an iPhone or iPad; elsewhere (the Simulator, whose
/// ad-hoc build has no Keychain entitlement, or a desktop working on the mobile interface) a file
/// beside the settings, so a test never writes to a desktop's keychain.
pub(crate) mod secrets {
    #[cfg(all(target_os = "ios", not(target_abi = "sim")))]
    const SERVICE: &str = "dev.benilla.launcher";

    #[cfg(all(target_os = "ios", not(target_abi = "sim")))]
    pub(crate) fn get(key: &str) -> Option<String> {
        let bytes = security_framework::passwords::get_generic_password(SERVICE, key).ok()?;
        String::from_utf8(bytes).ok()
    }

    #[cfg(all(target_os = "ios", not(target_abi = "sim")))]
    pub(crate) fn set(key: &str, value: &str) {
        if let Err(e) =
            security_framework::passwords::set_generic_password(SERVICE, key, value.as_bytes())
        {
            bevy::log::warn!("launcher: the Keychain refused a password: {e}");
        }
    }

    #[cfg(all(target_os = "ios", not(target_abi = "sim")))]
    pub(crate) fn delete(key: &str) -> bool {
        security_framework::passwords::delete_generic_password(SERVICE, key).is_ok()
    }

    /// The first iOS builds' single remembered login, taken out of the Keychain.
    #[cfg(all(target_os = "ios", not(target_abi = "sim")))]
    pub(crate) fn take_legacy() -> Option<(String, String)> {
        use security_framework::passwords as kc;
        let bytes = kc::get_generic_password("dev.benilla.login", "last").ok()?;
        let _ = kc::delete_generic_password("dev.benilla.login", "last");
        let text = String::from_utf8(bytes).ok()?;
        let (user, pass) = text.split_once('\n')?;
        Some((user.to_string(), pass.to_string()))
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    fn file() -> Option<std::path::PathBuf> {
        Some(crate::local_state::home()?.join("launcher-secrets.json"))
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    fn read_all() -> std::collections::BTreeMap<String, String> {
        file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    fn write_all(all: &std::collections::BTreeMap<String, String>) {
        if let (Some(p), Ok(text)) = (file(), serde_json::to_string(all)) {
            let _ = std::fs::write(p, text);
        }
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    pub(crate) fn get(key: &str) -> Option<String> {
        read_all().remove(key)
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    pub(crate) fn set(key: &str, value: &str) {
        let mut all = read_all();
        all.insert(key.to_string(), value.to_string());
        write_all(&all);
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    pub(crate) fn delete(key: &str) -> bool {
        let mut all = read_all();
        let had = all.remove(key).is_some();
        write_all(&all);
        had
    }

    #[cfg(not(all(target_os = "ios", not(target_abi = "sim"))))]
    pub(crate) fn take_legacy() -> Option<(String, String)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_is_found_by_address_and_made_when_new() {
        let mut s = Store::default();
        assert_eq!(s.profile_for("192.168.1.81"), 0);
        assert_eq!(s.profile_for("192.168.1.81"), 0);
        assert_eq!(s.profile_for("play.example.com:5000"), 1);
        assert_eq!(s.profiles[1].name, "play.example.com:5000");
    }

    #[test]
    fn accounts_are_case_blind() {
        let mut s = Store::default();
        let i = s.profile_for("host");
        assert_eq!(s.account_on(i, "Bilal"), Some(0));
        assert_eq!(s.account_on(i, "BILAL"), Some(0));
        assert_eq!(s.account_on(i, "other"), Some(1));
        assert_eq!(secret_key("Host", "bilal"), secret_key("host", "BILAL"));
    }

    #[test]
    fn the_store_round_trips_as_json() {
        let mut s = Store::default();
        let i = s.profile_for("host");
        s.profiles[i].realm_override = "1.2.3.4:8085".into();
        s.account_on(i, "a");
        s.last = Some((0, 0));
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Store>(&text).unwrap(), s);
    }
}
