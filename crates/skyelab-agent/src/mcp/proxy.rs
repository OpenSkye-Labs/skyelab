//! The stdio side: what `claude` runs as its MCP server.

use super::{SOCKET_ENV, TOKEN_ENV};
use anyhow::{Context as _, Result};
use std::io::{BufRead as _, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// `skyelab --mcp-proxy`: the socket and token come from the env the `--mcp-config` file sets.
pub fn proxy_main() -> Result<()> {
    let socket = std::env::var_os(SOCKET_ENV).context("SKYELAB_MCP_SOCKET is not set")?;
    let token = std::env::var(TOKEN_ENV).context("SKYELAB_MCP_TOKEN is not set")?;
    run_proxy(
        Path::new(&socket),
        &token,
        std::io::stdin(),
        std::io::stdout(),
    )
}

/// Sends `token`, then copies `input` to the server and the server's lines to `output` until the server
/// closes the connection.
pub fn run_proxy(
    socket: &Path,
    token: &str,
    mut input: impl Read + Send + 'static,
    mut output: impl Write,
) -> Result<()> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("connecting to SkyeLab at {}", socket.display()))?;
    writeln!(stream, "{token}").context("sending the token")?;
    let mut upstream = stream.try_clone().context("cloning the socket")?;
    std::thread::Builder::new()
        .name("skyelab-mcp-proxy-in".into())
        .spawn(move || {
            let _ = std::io::copy(&mut input, &mut upstream);
            // Tells the server no more requests come; it answers what it has, then closes.
            let _ = upstream.shutdown(std::net::Shutdown::Write);
        })
        .context("spawning the proxy's input thread")?;
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .context("reading from SkyeLab")?
            == 0
        {
            return Ok(());
        }
        output.write_all(&line).context("writing to claude")?;
        output.flush().context("writing to claude")?;
    }
}
