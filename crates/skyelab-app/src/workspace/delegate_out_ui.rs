//! The UI thread's half of a call to another machine: check the target, read the caller's login off the thread,
//! check again, send on the linked connection.

use super::{RemoteCall, Sent};
use crate::workspace::Workspace;
use gpui::{AppContext as _, Context, Window};
use serde_json::{Value, json};
use skyelab_core::AppSettings;

impl Workspace {
    /// Checks the target on the UI thread, reads the caller's login off it, checks again, and sends the call on the
    /// linked connection.
    pub(in crate::workspace) fn remote_out(
        &mut self,
        mut call: RemoteCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let refuse = |call: RemoteCall, why: String| _ = call.reply.send(Err(why));
        let checked = self
            .caller_live(&call.caller, cx)
            .and_then(|()| self.target(&call, cx));
        match checked {
            Err(why) => return refuse(call, why),
            Ok(Err(offline)) => {
                let _ = call.reply.send(Ok(Sent::Offline(offline)));
                return;
            }
            Ok(Ok(_)) => {}
        }
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == call.caller.account_id)
            .cloned()
        else {
            return refuse(call, "this thread's account isn't here any more".into());
        };
        let bin = self.backend.claude_bin.clone();
        let login = cx.background_spawn(async move { crate::login_cache::login(&bin, &account) });
        cx.spawn_in(window, async move |this, cx| {
            let login = login.await;
            let _ = this.update(cx, |this, cx| {
                // The caller may have stopped, or given up, while the login was read.
                if call.reply.is_disconnected() {
                    return;
                }
                let Some(login) = login else {
                    return refuse(
                        call,
                        "this thread's account has no Claude login with an email and organization \
                        that the other machine could check, so nothing was sent"
                            .into(),
                    );
                };
                call.delegator.login = skyelab_protocol::Login {
                    provider: "claude".into(),
                    email: login.email,
                    org_id: login.org_id,
                };
                match this
                    .caller_live(&call.caller, cx)
                    .and_then(|()| this.target(&call, cx))
                {
                    Ok(Ok((client, node))) => {
                        let pending = client.delegate(call.delegator.clone(), call.op.clone());
                        let _ = call.reply.send(Ok(Sent::Out(pending, node)));
                    }
                    Ok(Err(offline)) => _ = call.reply.send(Ok(Sent::Offline(offline))),
                    Err(why) => refuse(call, why),
                }
            });
        })
        .detach();
    }

    /// The linked connection to the call's machine, if this machine's agents may delegate there now: `Ok(Err)` when
    /// it isn't connected (nothing is sent), `Err` when it is refused.
    fn target(
        &self,
        call: &RemoteCall,
        cx: &gpui::App,
    ) -> Result<Result<(std::rc::Rc<skyelab_client::Client>, String), String>, String> {
        let machine = &call.machine;
        let on = self
            .env
            .settings_path
            .as_deref()
            .and_then(|p| AppSettings::load(p).ok())
            .is_some_and(|s| s.is_delegation_target(machine));
        if !on {
            return Err(format!(
                "delegation to {machine} is off; the user turns on \"Let my agents delegate here\" for it in Settings → Machines"
            ));
        }
        let env = self.linked_envs_of(machine, cx).into_iter().next();
        let env = env.ok_or_else(|| format!("{machine} isn't a linked machine here"))?;
        let env = env.read(cx);
        let Some(node) = env.node().map(str::to_owned) else {
            return Ok(Err(format!(
                "{machine} isn't connected ({}); nothing was sent",
                env.state.label()
            )));
        };
        if call.node.as_ref().is_some_and(|n| *n != node) {
            return Err(format!(
                "{machine} is another machine now than the one that thread was started on; nothing was sent"
            ));
        }
        if !env.speaks(skyelab_protocol::DELEGATION_SINCE_MINOR) {
            return Err(format!(
                "{machine} runs an older SkyeLab: update SkyeLab on {machine} to delegate there"
            ));
        }
        Ok(Ok((env.client(), node)))
    }

    /// The linked machines with delegation on, each `{machine, state, reason}`.
    pub(in crate::workspace) fn delegation_machines(&self, cx: &gpui::App) -> Vec<Value> {
        let settings = self
            .env
            .settings_path
            .as_deref()
            .and_then(|p| AppSettings::load(p).ok());
        let Some(settings) = settings else {
            return Vec::new();
        };
        self.env
            .linked
            .iter()
            .map(|l| l.env.read(cx))
            .filter(|e| settings.is_delegation_target(&e.host))
            .map(|e| {
                let (state, reason) =
                    match (e.node(), e.speaks(skyelab_protocol::DELEGATION_SINCE_MINOR)) {
                        (Some(_), true) => ("ready", String::new()),
                        (Some(_), false) => ("needs_update", "it runs an older SkyeLab".to_owned()),
                        (None, _) => ("offline", e.state.label().to_string()),
                    };
                json!({"machine": e.host, "state": state, "reason": reason})
            })
            .collect()
    }
}
