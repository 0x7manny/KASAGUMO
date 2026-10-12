# Architecture

Kasagumo is a small decentralized cloud: a set of peer nodes that place
container workloads on whoever has the most room, and keep content-addressed
blocks replicated across the cluster. There is no central coordinator — every
node runs the same daemon and they agree through gossip.

## Processes

Each machine runs one daemon, started with `kgo node start`. The daemon listens
on two sockets:

- a **Unix socket** (`<data-dir>/kgo.sock`), trusted and used by the local CLI;
- a **TCP port**, used by other nodes and guarded by TLS plus an identity proof.

The CLI (`kgo run`, `kgo ps`, …) is a thin client: it connects to the local
Unix socket and sends one JSON request. Cluster-wide actions (`Schedule`,
`Put`, `Get`, `Forward`) are only accepted from the local socket; a remote peer
that asks for one is refused.

## Identity and transport

See `src/identity.rs` and `src/secure.rs`.

- Every node owns an Ed25519 key. The node that runs `kgo cluster init` also
  holds the **cluster key**, which signs a certificate for each admitted node.
- Nodes talk over TLS 1.3 with an *unverified* ephemeral certificate — the TLS
  layer only provides confidentiality. Authentication is a separate step: each
  side signs the TLS session key (channel binding) with its node key and sends
  that signature plus its certificate. Because the signed message includes the
  session key and a role (`client`/`server`), a proof cannot be replayed onto
  another connection or the opposite direction.
- A peer that fails the proof is `Anonymous` and may only ask for `Info`.

## Connection origin and permissions

`dispatch` in `src/daemon.rs` gates every request on where it came from:

| Origin      | Allowed |
|-------------|---------|
| `Local`     | everything (the local CLI) |
| `Peer(ip)`  | everything except the CLI-only verbs (`Forward`, `Schedule`, `Put`, `Get`) |
| `Anonymous` | `Info` only |

## State

`src/store.rs` wraps a [redb](https://crates.io/crates/redb) database with five
tables:

- `workloads` — every workload this node was asked to run, and its state;
- `placements` — for a workload this node placed elsewhere, which peer runs it;
- `orphans` — a workload we relaunched after a node death, and where the old
  copy may still be, so we can stop it if that node comes back;
- `peers` — known peer addresses;
- `meta` — reserved.

Blocks live outside redb, one file per block under `<data-dir>/blobs`, named by
their SHA-256 (`src/blobs.rs`).

## Workload lifecycle

A workload moves through `Pending → Pulling → Running`, or to `Failed` if Docker
errors, or `Stopped` when asked. Transitions go through `Store::advance`, a
compare-and-set, so a stop that races a pull cannot be lost (`src/workload.rs`,
`launch` in `src/daemon.rs`). On daemon restart, `reconcile` walks the store and
marks anything that was mid-start, or whose container is gone, as `Failed`.

The container runtime is behind the `Runtime` trait, so the launch/reconcile
state machine can be tested without Docker. The real implementation
(`DockerRuntime`) shells out to the `docker` CLI; set `KGO_DOCKER` to point at a
different binary.

## Scheduling

`kgo run` on the CLI becomes a `Schedule` request. The daemon queries every node
for its free CPU and memory (`nodes`), keeps those that fit, and picks the one
with the most free CPU then memory, preferring the local node on a tie. If the
winner is remote, it forwards a `Run` and records a placement.

## Gossip, failure, and recovery

`monitor` runs every heartbeat interval:

- **heartbeat** — each node sends every peer a `Hello` carrying its listen port
  and the workloads it hosts, and learns the peer's peer list in return. New
  peers are verified and added; addresses that turn out to be ourselves are
  remembered and skipped.
- **death** — a peer silent for `DEAD_AFTER` heartbeats is declared dead. Its
  workloads are relaunched by the live node with the smallest id (the others
  forward what they know via `Adopt`), and a `Forget` tells everyone to stop
  tracking the old placement. A single `takeover` mutex ensures a workload is
  not relaunched twice.
- **return** — if a presumed-dead node comes back, whoever relaunched its
  workloads stops the duplicate copy (`release_orphans`).
- **repair** — every `REPAIR_EVERY` heartbeats, each node re-pushes its blocks
  to the nodes that should hold them.

## Block replication

Blocks are placed by rendezvous hashing: for a block id, nodes are ranked by
`sha256(block_id ++ node_id)` and the top `REPLICAS` should hold it. `Put`
writes to the top-ranked reachable nodes; `Get` tries locally then asks peers,
re-verifying the returned bytes against the id; `repair` restores the invariant
after a node joins or leaves.
