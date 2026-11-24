//! What the screens say about the connection.

use crate::source::Connection;
use gpui::SharedString;

/// The connection line under the title, and whether it is a problem.
pub(crate) fn connection_text(connection: &Connection) -> (SharedString, bool) {
    match connection {
        Connection::Connecting => ("Connecting…".into(), false),
        Connection::Connected => ("Connected".into(), false),
        Connection::Reconnecting(why) => (format!("Reconnecting… ({why})").into(), true),
        Connection::Refused => (
            "That machine didn't accept this device. Sign this phone in to your tailnet as the same user, \
             untagged, and check Tailscale is running on that machine."
                .into(),
            true,
        ),
        Connection::NodeChanged { was, now } => {
            let now = now.as_deref().unwrap_or("none");
            (
                format!(
                    "This isn't the machine you connected to before (node {now}, trusted: {was}). Trust it only \
                     if you replaced or renamed that machine."
                )
                .into(),
                true,
            )
        }
        Connection::Unverified(why) => (
            format!(
                "Can't check which machine this is ({why}). Nothing was sent to it. If the saved machines file \
                 is damaged, reinstall the app to forget them."
            )
            .into(),
            true,
        ),
        Connection::VersionMismatch(why) => (
            format!("That machine runs another SkyeLab version ({why}). Update both to the same release.")
                .into(),
            true,
        ),
        Connection::Stopped(why) => (
            format!("Disconnected ({why}). Go back and connect again.").into(),
            true,
        ),
    }
}
