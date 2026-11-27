//! Sharing & access's delegation buttons: Allow saves a link for the local account the request showed (refused
//! if that account no longer matches), Deny turns the caller down until sharing restarts, Revoke removes a link.
//! The account a request matches is read off the UI thread from each Claude account's login here.

use super::Workspace;
use crate::remote::delegation_panel::{
    AllowDelegation, CallerKey, DenyDelegation, RevokeDelegation, key_of,
};
use gpui::{AppContext as _, Context, Window};
use skyelab_core::delegation_remote::{DelegationLink, Login};
use skyelab_core::{AppSettings, Provider};
use std::collections::HashMap;

impl Workspace {
    /// Reads which local account matches each waiting request's login, when the requests changed.
    pub(super) fn match_delegation_requests(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.sharing() else {
            return;
        };
        let r = remote.read(cx);
        let unmatched: Vec<_> = r
            .delegation_requests
            .iter()
            .filter(|q| !r.delegation_matches.contains_key(&key_of(q)))
            .cloned()
            .collect();
        if unmatched.is_empty() {
            return;
        }
        let accounts: Vec<_> = self
            .config
            .accounts
            .iter()
            .filter(|a| a.provider == Provider::Claude)
            .cloned()
            .collect();
        let bin = self.backend.claude_bin.clone();
        let read = cx.background_spawn(async move {
            let logins: Vec<_> = accounts
                .iter()
                .filter_map(|a| crate::login_cache::login(&bin, a).map(|l| (a.clone(), l)))
                .collect();
            unmatched
                .iter()
                .map(|q| {
                    let wanted = Login {
                        provider: Provider::Claude,
                        email: q.email.to_ascii_lowercase(),
                        org_id: q.org_id.clone(),
                    };
                    let found = (q.provider == "claude")
                        .then(|| logins.iter().find(|(_, l)| *l == wanted))
                        .flatten()
                        .map(|(a, _)| (a.id.clone(), a.label.clone()));
                    (key_of(q), found)
                })
                .collect::<HashMap<CallerKey, _>>()
        });
        cx.spawn(async move |_, cx| {
            let found = read.await;
            remote.update(cx, |r, cx| {
                let now = std::time::Instant::now();
                r.delegation_checked
                    .extend(found.keys().map(|k| (k.clone(), now)));
                r.delegation_matches.extend(found);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn allow_delegation(
        &mut self,
        a: &AllowDelegation,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(remote) = self.sharing() else {
            return;
        };
        let key = (a.node_id.clone(), a.key_hash.clone(), a.account_id.clone());
        let r = remote.read(cx);
        let request = r
            .delegation_requests
            .iter()
            .find(|q| key_of(q) == key)
            .cloned();
        let matched = r.delegation_matches.get(&key).cloned().flatten();
        let path = r.settings_path().map(std::path::Path::to_path_buf);
        let (Some(request), Some((local, _)), Some(path)) = (request, matched, path) else {
            return;
        };
        let account = self.config.accounts.iter().find(|x| x.id == local).cloned();
        let (Some(account), true) = (account, local == a.local_account) else {
            return;
        };
        let link = DelegationLink {
            node_id: request.origin.node_id.clone(),
            key_hash: request.origin.key_hash.clone(),
            machine: request.origin.device.clone(),
            origin_account: request.account_id.clone(),
            login: Login {
                provider: Provider::Claude,
                email: request.email.to_ascii_lowercase(),
                org_id: request.org_id.clone(),
            },
            local_account: local,
        };
        // The account the request showed must be signed in to that login right now, read fresh (not cached).
        let bin = self.backend.claude_bin.clone();
        let saved = cx.background_spawn(async move {
            let now = skyelab_agent::login_identity(&bin, &account).ok().flatten();
            if now.as_ref() != Some(&link.login) {
                anyhow::bail!(
                    "{}'s login no longer matches that request; nothing was allowed",
                    account.id
                );
            }
            AppSettings::update(&path, |s| s.link_delegation(link))
                .map(|(s, ())| s.delegation.links)
        });
        cx.spawn(async move |_, cx| {
            let saved = saved.await;
            remote.update(cx, |r, cx| {
                match saved {
                    Ok(links) => {
                        r.delegation_links = links;
                        r.with_daemon(|d| d.forget_delegation_request(&key.0, &key.1, &key.2));
                        r.delegation_requests.retain(|q| key_of(q) != key);
                        r.approval_error = None;
                    }
                    Err(e) => r.approval_error = Some(format!("couldn't allow delegation: {e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn deny_delegation(
        &mut self,
        a: &DenyDelegation,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(remote) = self.sharing() {
            remote.update(cx, |r, cx| {
                r.with_daemon(|d| d.deny_delegation(&a.node_id, &a.key_hash, &a.account_id));
                r.delegation_requests.retain(|q| {
                    key_of(q) != (a.node_id.clone(), a.key_hash.clone(), a.account_id.clone())
                });
                cx.notify();
            });
        }
    }

    pub(super) fn revoke_delegation(
        &mut self,
        a: &RevokeDelegation,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(remote) = self.sharing() else {
            return;
        };
        let Some(path) = remote
            .read(cx)
            .settings_path()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let saved = AppSettings::update(&path, |s| {
            s.revoke_delegation(&a.node_id, &a.key_hash, &a.account_id);
        });
        remote.update(cx, |r, cx| {
            match saved {
                Ok(s) => {
                    r.delegation_links = s.0.delegation.links;
                    r.approval_error = None;
                }
                Err(e) => {
                    r.approval_error = Some(format!("couldn't revoke the delegation link: {e:#}"))
                }
            }
            cx.notify();
        });
    }
}
