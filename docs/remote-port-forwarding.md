# Remote environments: port forwarding

Part of [remote.md](remote.md).

**Port forwarding (1.5).** A connected device may also forward a port on this machine's loopback to itself, so a
dev server an agent started here opens in a browser there. The main connection asks with `forward_port {port}` and
gets a random token for that forward. The daemon refuses ports below 1024, its own listening ports, and a denylist
of well-known ports whose services hand over the machine, its user or their data without a login of their own (or
with one that trusts loopback, which is exactly where a forward lands):

- Remote control of the machine or a device: 3389 Remote Desktop, 5900/5901 VNC, 6000 X11, 5037 adb server, 5555 an
  Android device's adb.
- Code execution: 2375/2376 Docker's API, 6443 Kubernetes API, 10250 kubelet (exec in pods), 8001 kubectl proxy,
  9000 php-fpm (FastCGI runs any PHP file it is pointed at), 8888 Jupyter, 9222 browser debugging, 9229 Node
  inspector, 4444 WebDriver (Selenium, geckodriver) and 9515 ChromeDriver (both start a browser with any flags),
  8500 Consul (its API registers checks that run scripts), 8983 Solr.
- Data stores and queues that ship with no login or a loopback-only default one: 2379/2380 etcd, 2181 ZooKeeper,
  3306 MySQL, 5432 Postgres, 5984 CouchDB, 6379 Redis, 8086 InfluxDB, 9042 Cassandra, 9092 Kafka, 9200/9300
  Elasticsearch, 11211 memcached, 15672 RabbitMQ's management UI (its `guest` login works only from localhost),
  27017 MongoDB.
- Network pivots: 1080 SOCKS proxy (it would let the other device reach this machine's network as this machine).

Generic dev-server ports (3000, 4200, 5000, 5173, 8000, 8080, …) stay forwardable: forwarding them is the point. A
service moved to another port isn't caught; the list only stops the well-known defaults.

The daemon also refuses more than 32 forwards per connection, and a port nothing accepts on 127.0.0.1 or ::1 (it dials once to check). It never dials any other host.

Each TCP connection the client accepts locally opens **a new connection to the same listener**, through the whole
gate again (whois, TLS, no `Origin`, approved key in hello). That hello declares the `tunnel` capability. A tunnel
connection gets no pushed events, isn't listed as a connected device, and may call only `open_tunnel
{port, token}`: anything else is refused and closes it. That rule keeps a hidden connection from forwarding or
driving anything out of Disconnect's reach. `open_tunnel` is admitted only while another live, non-tunnel connection
with **the same approval** (node and key hash) holds the forward that token names. So a port nobody asked for, or one
another device, key or window asked for, is never reached. After its `ok`, the connection carries raw bytes in binary
frames to and from that port. No new listener is opened on the tailnet.

A tunnel ends when its forward does (`stop_forward {port, token}`, or the owning connection closes, is revoked or
Disconnected), when it is kicked itself, on EOF from either side, or when a write stalls past the idle timeout.
Each forward has at most 64 tunnels.

The client (`Client::forward_port`) binds 127.0.0.1 only, through `skyelab_core::process::exclusive`. It keeps the
remote port number when nothing here answers on it at 127.0.0.1 or ::1, else it takes any free port. A connect
check is needed because std sets SO_REUSEADDR, and on macOS that lets a bind to 127.0.0.1:P succeed over a local
server on the wildcard address. The desktop opens and copies `http://127.0.0.1:<port>/`, the socket the client owns,
never `localhost`, which a browser may send to whatever holds ::1. A browser tab opened at a forward's address (also a Duplicate of
one) gets a private, in-memory web store of its own, as an agent's tab does: cookies ignore the port, so on the shared
store the other machine's server would get this machine's 127.0.0.1 cookies, and a reused local port would share
storage and service workers between machines. A unique origin per forward isn't possible: macOS has only 127.0.0.1
on its loopback, and `*.localhost` may resolve to ::1.

Accepted limits, as with `ssh -L`: any local process and any page in a browser here can connect to the local
port (the forwarded server's own Host checks guard against DNS rebinding). On Windows, the listener isn't bound with
`SO_EXCLUSIVEADDRUSE` (std can't set it, and `unsafe` belongs in a `-sys` crate), so a same-user process could bind
the port over it; a same-user process there can already read the device key (remote.md). The client dials at most 3 tunnels at a time
across all its forwards, because the gate admits 4 pending per address and drops the rest. It keeps at most 64
open per forward. A tunnel's hello must name the node the approved main connection named, and the client skips
any pushed event while it waits for the `open_tunnel` reply. A port is forwarded once per client.

Dropping the `Forward`, or the main connection ending, closes the listener and its tunnels. Dropping it also sends
`stop_forward`, and so does a forward that the server allowed but the client couldn't start. A `forward_port`
request never times out on the client, so a late reply can't leave a forward that nobody stops (the daemon's probe
bounds it to 10 s). The desktop also drops every forward on a refusal, on a machine switch and when the window
closes. Half-close isn't carried: EOF on either side closes the tunnel. A client checks `hello.protocol.minor >= 5`
and never sends the calls to an older server, and the desktop palette says that machine is too old. An older client
never sends them.
