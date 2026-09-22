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
# Start a node
kasagumo node start

# List available nodes
kasagumo nodes

# Run a workload
kasagumo run --cpu 2 --memory 4GB nginx:latest

# List running workloads
kasagumo ps

# Stop a workload
kasagumo stop <workload-id>
