//! Each account's Claude login identity (`skyelab_agent::login_identity`), read at most once per 10 minutes per
//! config dir, for the delegation checks on both machines. Only an identity that was read is kept: an unread or
//! unprovable login is asked again next time, and always refuses meanwhile.

use skyelab_core::Account;
use skyelab_core::delegation_remote::Login;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_secs(600);

type Cache = Mutex<HashMap<(PathBuf, String), (Instant, Login)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Cache::default)
}

/// `account`'s login identity, `None` when it can't be read or proves nothing. Blocking: call it off the UI thread.
pub fn login(bin: &Path, account: &Account) -> Option<Login> {
    let key = (account.config_dir.clone(), account.id.clone());
    if let Some((at, login)) = cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        && at.elapsed() < FRESH
    {
        return Some(login.clone());
    }
    let login = match skyelab_agent::login_identity(bin, account) {
        Ok(Some(login)) if login.is_provable() => login,
        Ok(_) => return None,
        Err(e) => {
            eprintln!("skyelab: can't read {}'s login: {e:#}", account.id);
            return None;
        }
    };
    cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, (Instant::now(), login.clone()));
    Some(login)
}
