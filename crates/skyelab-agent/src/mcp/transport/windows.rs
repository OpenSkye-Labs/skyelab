//! A named pipe, `\\.\pipe\skyelab-<pid>-<64 random hex>`, that only this user can open (a protected DACL naming
//! the user's SID, from `skyelab_pipe_sys`), local clients only, and created as the first instance so a name someone
//! squatted fails the start. tokio's overlapped pipes, blocked on from plain threads: a synchronous pipe handle
//! serializes its I/O, so one thread's blocked read would stall another thread's write on the same pipe.

use anyhow::{Context as _, Result};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use tokio::runtime::Runtime;
use tokio::sync::watch;

/// `ERROR_PIPE_BUSY`: every instance is connected; the server makes the next one right after an accept.
const ERROR_PIPE_BUSY: i32 = 231;
/// How long a connect waits for a free instance.
const BUSY_WAIT: Duration = Duration::from_secs(5);

fn runtime() -> io::Result<&'static Runtime> {
    static RT: OnceLock<Runtime> = OnceLock::new();
    if let Some(rt) = RT.get() {
        return Ok(rt);
    }
    // Multi-thread: only that kind drives I/O for `block_on` called from many plain threads at once.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("skyelab-mcp-pipe")
        .enable_all()
        .build()?;
    Ok(RT.get_or_init(|| rt))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding it leaves plain data behind, still consistent.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

enum Pipe {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}

impl Pipe {
    async fn readable(&self) -> io::Result<()> {
        match self {
            Pipe::Server(p) => p.readable().await,
            Pipe::Client(p) => p.readable().await,
        }
    }

    fn try_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Pipe::Server(p) => p.try_read(buf),
            Pipe::Client(p) => p.try_read(buf),
        }
    }

    async fn writable(&self) -> io::Result<()> {
        match self {
            Pipe::Server(p) => p.writable().await,
            Pipe::Client(p) => p.writable().await,
        }
    }

    fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Pipe::Server(p) => p.try_write(buf),
            Pipe::Client(p) => p.try_write(buf),
        }
    }
}

/// One end of a connected pipe, with `UnixStream`'s methods. Clones share the pipe, the read timeout and the cut.
pub(crate) struct Conn {
    pipe: Arc<Pipe>,
    /// Set by [`Conn::shutdown`]: wakes blocked reads (with EOF) and writes, on either end.
    closed: Arc<watch::Sender<bool>>,
    read_timeout: Arc<Mutex<Option<Duration>>>,
}

impl Conn {
    fn new(pipe: Pipe) -> Conn {
        Conn {
            pipe: Arc::new(pipe),
            closed: Arc::new(watch::channel(false).0),
            read_timeout: Arc::default(),
        }
    }

    pub(crate) fn try_clone(&self) -> io::Result<Conn> {
        Ok(Conn {
            pipe: self.pipe.clone(),
            closed: self.closed.clone(),
            read_timeout: self.read_timeout.clone(),
        })
    }

    pub(crate) fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        if timeout == Some(Duration::ZERO) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a zero timeout",
            ));
        }
        *lock(&self.read_timeout) = timeout;
        Ok(())
    }

    /// Cuts the connection both ways, whatever `_how` says: a pipe has no half-close. The server end also
    /// disconnects the client.
    pub(crate) fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
        self.closed.send_replace(true);
        match &*self.pipe {
            Pipe::Server(p) => p.disconnect(),
            Pipe::Client(_) => Ok(()),
        }
    }

    fn block_on<T>(&self, f: impl Future<Output = io::Result<T>>) -> io::Result<T> {
        runtime()?.block_on(f)
    }
}

impl io::Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = *lock(&self.read_timeout);
        let mut closed = self.closed.subscribe();
        let pipe = &self.pipe;
        let read = async {
            loop {
                tokio::select! {
                    ready = pipe.readable() => ready?,
                    _ = closed.wait_for(|c| *c) => return Ok(0),
                }
                match pipe.try_read(buf) {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    // A client that hung up reads as EOF, as a closed socket does.
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(0),
                    read => return read,
                }
            }
        };
        self.block_on(async {
            match timeout {
                Some(t) => tokio::time::timeout(t, read)
                    .await
                    .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into())),
                None => read.await,
            }
        })
    }
}

impl io::Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut closed = self.closed.subscribe();
        let pipe = &self.pipe;
        self.block_on(async {
            loop {
                tokio::select! {
                    ready = pipe.writable() => ready?,
                    _ = closed.wait_for(|c| *c) => return Err(io::ErrorKind::BrokenPipe.into()),
                }
                match pipe.try_write(buf) {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    written => return written,
                }
            }
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Hands out connected pipe instances, keeping one waiting for the next client.
pub(crate) struct Listener {
    name: PathBuf,
    next: Mutex<Option<NamedPipeServer>>,
}

impl Listener {
    /// Waits for a client on the waiting instance, then makes the next one. Its `()` stands for unix's peer address.
    pub(crate) fn accept(&self) -> io::Result<(Conn, ())> {
        let waiting = lock(&self.next).take();
        let server = match waiting {
            Some(s) => s,
            None => create(&self.name, false)?,
        };
        let connected = runtime()?.block_on(server.connect());
        *lock(&self.next) = create(&self.name, false).ok();
        connected?;
        Ok((Conn::new(Pipe::Server(server)), ()))
    }
}

fn create(name: &Path, first: bool) -> io::Result<NamedPipeServer> {
    let _runtime = runtime()?.enter();
    skyelab_pipe_sys::create_private_pipe(
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true),
        name.as_os_str(),
    )
}

/// The run dir lives in the user's profile (the platform data dir), whose inherited ACL already keeps other users
/// out; grant files in it hold tokens, like the rest of the user's app data.
pub(crate) fn private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
}

/// A new file (in [`private_dir`]), failing if it exists.
pub(crate) fn create_private_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Listens on a fresh pipe named after this process and `random`. No file: the address is the pipe's name.
pub(crate) fn bind(_run_dir: &Path, random: &str) -> Result<(Listener, PathBuf, Option<PathBuf>)> {
    let name = PathBuf::from(format!(r"\\.\pipe\skyelab-{}-{random}", std::process::id()));
    let first = create(&name, true).with_context(|| format!("listening on {}", name.display()))?;
    let listener = Listener {
        name: name.clone(),
        next: Mutex::new(Some(first)),
    };
    Ok((listener, name, None))
}

pub(crate) fn connect(address: &Path) -> io::Result<Conn> {
    let _runtime = runtime()?.enter();
    let deadline = Instant::now() + BUSY_WAIT;
    loop {
        match ClientOptions::new().open(address) {
            Ok(client) => return Ok(Conn::new(Pipe::Client(client))),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

/// A pipe goes with its last handle; nothing to remove.
pub(crate) fn cleanup(_address: &Path, _socket_dir: Option<&Path>) {}

pub(crate) fn alive(pid: u32) -> bool {
    skyelab_pipe_sys::process_alive(pid)
}
