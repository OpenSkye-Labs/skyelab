//! Sharing & access → delegation from the user's other machines: the callers this machine refused for want of a
//! link (Allow, Deny), and the links it allows (Revoke). A request whose login matches no account here says why and
//! has no Allow. The buttons dispatch actions the Workspace handles, so SKYELAB_VERIFY_ACTIONS can drive them.

use super::Remote;
use crate::ui;
use crate::ui::Button;
use gpui::{Action, AnyElement, SharedString, div, prelude::*};
use gpui_component::{Theme, v_flex};
use skyelab_daemon::DelegationRequest;

/// A refused caller: (origin node, device key hash, its account there).
pub type CallerKey = (String, String, String);

pub fn key_of(r: &DelegationRequest) -> CallerKey {
    (
        r.origin.node_id.clone(),
        r.origin.key_hash.clone(),
        r.account_id.clone(),
    )
}

/// Allows agents on that machine under that account to start threads here in folders bound to `local_account`,
/// the account shown on the request when the user clicked.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct AllowDelegation {
    pub node_id: String,
    pub key_hash: String,
    pub account_id: String,
    pub local_account: String,
}

/// Refuses that caller, unlisted, until sharing restarts.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct DenyDelegation {
    pub node_id: String,
    pub key_hash: String,
    pub account_id: String,
}

/// Removes a saved link.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct RevokeDelegation {
    pub node_id: String,
    pub key_hash: String,
    pub account_id: String,
}

fn short(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// "Agents on <machine> under <account> (<email> · <org>)": the caller's account by its label there, or by its id
/// when an older peer sent no label.
pub(crate) fn who(r: &DelegationRequest) -> String {
    // Every field but the device name comes from the other machine's message.
    let clean = skyelab_core::delegation_remote::clean_title;
    let account = r
        .account_label
        .as_deref()
        .map(clean)
        .filter(|l| !l.is_empty());
    format!(
        "Agents on {} under {} ({} · {})",
        short(&r.origin.device),
        account.unwrap_or_else(|| clean(&r.account_id)),
        clean(&r.email),
        clean(&r.org_id)
    )
}

fn button(id: String, label: &'static str, action: Box<dyn Action>) -> Button {
    Button::new(SharedString::from(id))
        .small()
        .label(label)
        .on_click(move |_, window, cx| crate::workspace::dispatch(action.boxed_clone(), window, cx))
}

fn requests(remote: &Remote, theme: &Theme) -> Vec<AnyElement> {
    remote
        .delegation_requests
        .iter()
        .enumerate()
        .map(|(n, r)| {
            let key = key_of(r);
            let id = format!("{}-{}-{}", key.0, key.1, key.2);
            let local = remote.delegation_matches.get(&key).cloned().flatten();
            let who = who(r);
            let what: SharedString = match &local {
                Some((_, label)) => {
                    format!("{who} want to start threads in folders bound to {label} here").into()
                }
                None if remote.delegation_matches.contains_key(&key) => format!(
                    "{who}: no account here is signed in to that Claude login, so it can't be allowed"
                )
                .into(),
                None => format!("{who}: checking this machine's accounts…").into(),
            };
            let deny = DenyDelegation {
                node_id: key.0.clone(),
                key_hash: key.1.clone(),
                account_id: key.2.clone(),
            };
            ui::group_row(n == 0, theme)
                .id(SharedString::from(format!("delegation-request-{id}")))
                .child(ui::StatusDot::new(theme.warning))
                .child(div().flex_1().min_w_0().child(what))
                .children(local.map(|(local_account, _)| {
                    let allow = AllowDelegation {
                        node_id: key.0.clone(),
                        key_hash: key.1.clone(),
                        account_id: key.2.clone(),
                        local_account,
                    };
                    button(format!("allow-delegation-{id}"), "Allow", Box::new(allow)).primary()
                }))
                .child(button(format!("deny-delegation-{id}"), "Deny", Box::new(deny)))
                .into_any_element()
        })
        .collect()
}

fn links(remote: &Remote, theme: &Theme) -> Vec<AnyElement> {
    if remote.delegation_links.is_empty() {
        return vec![
            ui::group_row(true, theme)
                .child(ui::EmptyState::new("No machines may delegate here").hint(
                    "When an agent on another of your machines asks to start threads here, its request shows above.",
                ))
                .into_any_element(),
        ];
    }
    remote
        .delegation_links
        .iter()
        .enumerate()
        .map(|(n, l)| {
            let revoke = RevokeDelegation {
                node_id: l.node_id.clone(),
                key_hash: l.key_hash.clone(),
                account_id: l.origin_account.clone(),
            };
            let id = format!("{}-{}-{}", l.node_id, l.key_hash, l.origin_account);
            ui::group_row(n == 0, theme)
                .id(SharedString::from(format!("delegation-link-{id}")))
                .child(div().flex_1().min_w_0().truncate().child(format!(
                    "{} under {} → {} here",
                    short(&l.machine),
                    l.origin_account,
                    l.local_account
                )))
                .child(ui::mono(
                    format!("{} · {}", l.login.email, l.login.org_id),
                    theme,
                ))
                .child(
                    button(
                        format!("revoke-delegation-{id}"),
                        "Revoke",
                        Box::new(revoke),
                    )
                    .ghost(),
                )
                .into_any_element()
        })
        .collect()
}

/// The two delegation sections of Sharing & access.
pub fn sections(remote: &Remote, theme: &Theme) -> Vec<AnyElement> {
    let section = |title: &'static str, rows: Vec<AnyElement>| {
        v_flex()
            .gap_2p5()
            .child(ui::subsection_title(title, theme))
            .child(ui::group(theme).children(rows))
            .into_any_element()
    };
    let waiting = requests(remote, theme);
    let mut out = Vec::new();
    if !waiting.is_empty() {
        out.push(section("Delegation requests", waiting));
    }
    out.push(section("Allowed delegation", links(remote, theme)));
    out
}

#[cfg(test)]
#[path = "delegation_panel_test.rs"]
mod tests;
