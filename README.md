<p align="center">
  <img src="./assets/kasagumo-logo.png" width="420" alt="Kasagumo Logo">
</p>

<p align="center">
  A decentralized cloud built in Rust.
</p>

<p align="center">
  <a href="https://github.com/0x7manny/KASAGUMO/actions/workflows/ci.yml"><img src="https://github.com/0x7manny/KASAGUMO/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
</p>

---

## About

**Kasagumo** is a decentralized cloud where machines can share their computing resources.

- **Run** containers on the node with the most free CPU and memory.
- **Store** files as blocks copied on several nodes.
- **Recover** on its own: when a node goes down, its workloads and blocks move to the others.

Every node has its own key. Nodes talk over TLS and only accept members of their cluster. Built with **Rust**.

## Name

**Kasagumo (笠雲)** is a Japanese word meaning **"cap cloud"**.

It describes a cloud that forms like a hat over **Mount Fuji**.

## Quick start

You need [Rust](https://rustup.rs) and [Docker](https://www.docker.com).

```bash
cargo install --path .

# First machine: create the cluster, start a node
kgo cluster init
kgo node start --port 7070

# Every other machine: get admitted by the first one
kgo node id                       # prints this machine's public key
kgo cluster admit <public-key>    # on the first machine: prints a certificate
kgo node enroll <certificate>     # back on this machine
kgo node start --port 7070
kgo node join 192.168.1.10:7070   # nodes then find each other by themselves

# Run a workload: it is placed on the best node
kgo run nginx:latest
kgo nodes
```

## Commands

```bash
kgo cluster init                              # create a cluster (first node)
kgo cluster admit <public-key>                # admit a node (where you ran init)
kgo node id                                   # show this node's public key
kgo node enroll <certificate>                 # join a cluster
kgo node start --port 7070                    # start a node
kgo node join <addr>                          # add a peer
kgo nodes                                     # list nodes, free CPU and memory

kgo run --cpu 2 --memory 4GB nginx:latest     # run a workload
kgo run -p 8080:80 -e MODE=prod nginx:latest  # publish a port, set a variable
kgo --on <addr> run nginx:latest              # run on a given node (also ps)

kgo ps [--all]                                # list workloads
kgo logs <workload-id>                        # show its output
kgo stop <workload-id>                        # stop it

kgo put ./photo.jpg                           # store a file, prints its id
kgo get <file-id> ./photo-copy.jpg            # get it back
```

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Tests start real nodes on your machine and replace Docker with a fake one, so they run without Docker.
