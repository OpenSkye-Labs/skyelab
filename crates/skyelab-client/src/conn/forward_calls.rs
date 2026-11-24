//! The main connection's side of port forwarding: ask the environment, then start the local listener.

use std::sync::PoisonError;

use skyelab_protocol::{Call, ErrorCode, Reply, RpcError};

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
        let old = cx
            .hello
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_none_or(|h| h.protocol.minor < 5);
        let refused = |message: String| {
            let _ = reply.send(Err(Error::Rpc(RpcError::new(ErrorCode::NotWired, message))));
            Ok(())
        };
        if old {
            return refused(format!(
                "{} runs an older SkyeLab without port forwarding",
                cx.host
            ));
        }
        let taken = !cx
            .forwarding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(port);
        if taken {
            return refused(format!("port {port} is already forwarded"));
        }
        let call = Call::ForwardPort { port };
        self.request(cx, call, Waiter::Forward(port, reply)).await
    }

    /// The environment answered `forward_port`.
    pub(super) fn forwarded(
        &self,
        cx: &Ctx,
        port: u16,
        reply: flume::Sender<Result<Forward, Error>>,
        result: Result<Reply, Error>,
    ) {
        let forward = match result {
            Ok(Reply::Ok) => self.start_forward(cx, port),
            Ok(_) => Err(Error::UnexpectedReply),
            Err(e) => Err(e),
        };
        if forward.is_err() {
            cx.forwarding
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&port);
        }
        // A caller that gave up drops the forward here, which stops it.
        let _ = reply.send(forward);
    }

    /// The environment allowed `port`: listen on this machine's loopback and tunnel each connection to it.
    fn start_forward(&self, cx: &Ctx, port: u16) -> Result<Forward, Error> {
        let node = cx
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
        let route = forward::Route {
            target: cx.target(),
            node,
            remote_port: port,
            session: self.session.subscribe(),
            commands: cx.commands_tx.clone(),
            forwarding: cx.forwarding.clone(),
        };
        forward::start(listener, route).map_err(cant_listen)
    }
}
