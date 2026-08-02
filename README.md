<p align="center">
  <img src="./assets/kasagumo-logo.png" width="420" alt="Kasagumo Logo">
</p>

<p align="center">
  A decentralized cloud built in Rust.
</p>

---

## About

**Kasagumo** is a decentralized cloud where machines can share their computing resources.

Built with **Rust**.

## Name

**Kasagumo (笠雲)** is a Japanese word meaning **"cap cloud"**.

It describes a cloud that forms like a hat over **Mount Fuji**.

## Commands

```bash
# Start a node (Unix socket for the CLI, TCP port for peers)
kgo node start --port 7070

# Add a peer to this node
kgo node join 192.168.1.10:7070

# List available nodes
kgo nodes

# Run a workload
kgo run --cpu 2 --memory 4GB nginx:latest

# List running workloads
kgo ps

# Stop a workload
kgo stop <workload-id>
