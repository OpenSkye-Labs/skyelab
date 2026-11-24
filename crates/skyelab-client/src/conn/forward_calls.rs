//! The main connection's side of port forwarding: ask the environment, then start the local listener.

use std::sync::PoisonError;

use skyelab_protocol::{Call, ErrorCode, FORWARDS_SINCE_MINOR, Reply, RpcError};

use super::forward::{self, Forward};
use super::{Ctx, End, Live, Waiter};
use crate::Error;

impl Live {
    /// `forward_port`, unless the environment is older than 1.5 or this client already forwards `port`.
    pub(super) async fn forward(
        &mut self,
        cx: &mut Ctx,
        port: u16,
        reply: flume::Sender<Result<Forward, Error>>,
    ) -> Result<(), End> {
        let new_enough = cx
            .hello
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|h| h.protocol.minor >= FORWARDS_SINCE_MINOR);
        let refused = |message: String| {
            let _ = reply.send(Err(Error::Rpc(RpcError::new(ErrorCode::NotWired, message))));
            Ok(())
        };
        if !new_enough {
            return refused(format!(
                "{} runs an older SkyeLab without port forwarding",
                cx.host
            ));
        }
        let Some(reservation) = cx.forwarding.claim(port) else {
            return refused(format!("port {port} is already forwarded"));
        };
        let call = Call::ForwardPort { port };
        self.request(cx, call, Waiter::Forward(port, reservation, reply))
            .await
    }

    /// The environment answered `forward_port`. A forward that can't start here is stopped there too.
    pub(super) fn forwarded(
        &self,
        cx: &Ctx,
        port: u16,
        reservation: u64,
        reply: flume::Sender<Result<Forward, Error>>,
        result: Result<Reply, Error>,
    ) {
        let forward = match result {
            Ok(Reply::Forwarded { token }) => {
                let started = self.start_forward(cx, port, reservation, &token);
                if started.is_err() {
                    forward::stop_remote(&cx.commands_tx, port, &token);
                }
                started
            }
            Ok(_) => Err(Error::UnexpectedReply),
            Err(e) => Err(e),
        };
        if forward.is_err() {
            cx.forwarding.release(port, reservation);
        }
        // A caller that gave up drops the forward here, which stops it.
        let _ = reply.send(forward);
    }

    fn start_forward(
        &self,
        cx: &Ctx,
        port: u16,
        reservation: u64,
        token: &str,
    ) -> Result<Forward, Error> {
        let approved_node = cx
            .hello
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|h| h.node_id.clone());
        let cant_listen = |e: std::io::Error| {
            Error::Rpc(RpcError::new(
                ErrorCode::Internal,
                format!("can't listen on this machine: {e}"),
            ))
        };
        let listener = forward::bind_local(port).map_err(cant_listen)?;
        let mut target = cx.target();
        target.tunnel = true;
        let route = forward::Route {
            target,
            approved_node,
            remote_port: port,
            token: token.to_owned(),
            reservation,
            session_ended: self.session_ended.subscribe(),
            dials: cx.dials.clone(),
            commands: cx.commands_tx.clone(),
            forwarding: cx.forwarding.clone(),
        };
        forward::start(listener, route).map_err(cant_listen)
    }
}
