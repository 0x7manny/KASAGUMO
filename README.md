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
kgo node start --port 7070 --token <secret>   # or KGO_TOKEN

# Add a peer to this node (peers then discover each other by heartbeat;
# workloads of a peer that goes down are rescheduled elsewhere)
kgo node join 192.168.1.10:7070

# List available nodes
kgo nodes

# Run a workload (placed on the node with the most free CPUs)
kgo run --cpu 2 --memory 4GB nginx:latest

# Run on a peer (same token on both nodes)
kgo --on 192.168.1.10:7070 run nginx:latest

# List running workloads (also --on)
kgo ps

# Show the output of a workload (stop and logs find the hosting node by themselves)
kgo logs <workload-id>

# Store a file on the cluster (2 copies per block) and get it back by its id
kgo put ./photo.jpg
kgo get <file-id> ./photo-copy.jpg

# Stop a workload
kgo stop <workload-id>
