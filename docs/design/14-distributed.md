# 14 - Distributed Runtime

## Overview

The Runtime is designed to scale from a single process to multiple distributed nodes. The same binary can be deployed as different roles in a distributed architecture.

## Single Process → Distributed

```
Phase 1: Single Process
┌──────────────────────────────┐
│       Runtime (all-in-one)   │
│  TCP + UDP + HTTP + Plugins  │
└──────────────────────────────┘

Phase 2: Multiple Processes
┌──────────┐  ┌──────────┐  ┌──────────┐
│ Gateway  │  │ Zone #1  │  │ Zone #2  │
│ Runtime  │  │ Runtime  │  │ Runtime  │
└──────────┘  └──────────┘  └──────────┘

Phase 3: Orchestrated
┌─────────────────────────────────────────┐
│  Kubernetes / Docker Swarm              │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐  │
│  │ Gateway │ │ Gateway │ │ Gateway │  │
│  └────┬────┘ └────┬────┘ └────┬────┘  │
│       └──────────┼──────────┘         │
│            ┌─────┴─────┐              │
│            │ Zone Pods  │              │
│            └───────────┘              │
└─────────────────────────────────────────┘
```

## Node Types

| Node Type | Role | Capabilities |
|-----------|------|--------------|
| Gateway | Client routing, auth | TCP Listener, UDP Listener, Auth, Routing |
| Game Zone | Game world hosting | TCP Listener, UDP Listener, Plugin Runtime, World State |
| Chat | Chat messaging | TCP Listener, Pub/Sub |
| Matchmaking | Player matching | TCP Listener, Queue Management |
| Worker | Background tasks | Plugin Runtime, Scheduler |
| Web API | HTTP endpoints | HTTP Server, Database |

## Inter-Node Communication

### Message Format

Nodes communicate using the same protocol as client-server:

```rust
pub struct NodeMessage {
    pub source_node: String,
    pub target_node: String,
    pub message_type: NodeMessageType,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

pub enum NodeMessageType {
    // State sync
    PlayerUpdate,
    WorldUpdate,

    // Transfer
    PlayerTransferRequest,
    PlayerTransferResponse,
    PlayerTransferComplete,

    // Discovery
    NodeHello,
    NodeHeartbeat,
    NodeGoodbye,

    // Pub/Sub
    Publish,
    Subscribe,
    Unsubscribe,
}
```

### TCP Connections Between Nodes

```
Node A (Zone 1)
    │
    │── TCP Connection ──→ Node B (Zone 2)
    │
    │── TCP Connection ──→ Node C (Gateway)
    │
    └── Persistent connections with heartbeat
```

### Node Discovery

```toml
[nodes]
# Static configuration
[[nodes.instances]]
id = "zone-1"
address = "10.0.1.10:7780"
type = "zone"
zone = "starter"

[[nodes.instances]]
id = "zone-2"
address = "10.0.1.11:7780"
type = "zone"
zone = "dungeon"

# Dynamic discovery
[nodes.discovery]
method = "dns"  # dns | consul | kubernetes
dns_record = "game-nodes.internal"
refresh_interval = 10
```

## Player Transfer

When a player moves between zones:

```
Player → Zone 1 → Gateway → Zone 2

1. Player issues move command in Zone 1
2. Zone 1 determines destination is in Zone 2
3. Zone 1 sends TransferRequest to Gateway
4. Gateway forwards to Zone 2
5. Zone 2 reserves player slot
6. Zone 2 accepts transfer
7. Gateway tells Zone 1 to release player
8. Zone 1 serializes player state
9. State sent to Zone 2
10. Zone 2 deserializes and activates player
11. Client notified of transfer
```

```rust
pub struct PlayerTransfer {
    pub player_id: u64,
    pub from_node: String,
    pub to_node: String,
    pub player_state: Vec<u8>,  // Serialized player data
    pub inventory_state: Vec<u8>,
    pub session_state: Vec<u8>,
}

impl GatewayRuntime {
    pub async fn handle_transfer(&self, transfer: PlayerTransfer) -> Result<()> {
        // 1. Verify source and destination exist
        let source = self.server_registry.get(&transfer.from_node)?;
        let dest = self.server_registry.get(&transfer.to_node)?;

        // 2. Request transfer
        self.send_to_node(&transfer.to_node, NodeMessage::transfer_request(&transfer)).await?;

        // 3. Wait for confirmation
        let response = self.wait_for_response(&transfer.to_node, timeout: 5s).await?;

        if response.accepted {
            // 4. Forward player state
            self.send_to_node(&transfer.to_node, NodeMessage::transfer_data(&transfer)).await?;

            // 5. Notify client
            self.notify_client_of_transfer(transfer.player_id, &transfer.to_node).await?;

            // 6. Update routing
            self.router.update_player_route(transfer.player_id, &transfer.to_node)?;
        }

        Ok(())
    }
}
```

## State Synchronization

### Full State Sync (On Transfer)

```rust
pub struct PlayerStateSnapshot {
    pub character: Character,
    pub inventory: Inventory,
    pub equipment: Equipment,
    pub combat_state: Option<Combat>,
    pub active_effects: Vec<Effect>,
    pub quest_progress: Vec<QuestProgress>,
}
```

### Incremental Sync (During Gameplay)

```rust
pub enum StateDelta {
    PlayerMoved { player_id: u64, room_id: u32 },
    PlayerDamaged { player_id: u64, damage: u32 },
    ItemAcquired { player_id: u64, item_id: u32 },
    ItemRemoved { player_id: u64, item_id: u32 },
    // ...
}
```

### Sync Strategy

```
Zone 1                    Zone 2
  │                         │
  │── StateDelta ──────────→│  (for shared players)
  │                         │
  │── StateDelta ──────────→│
  │                         │
  │←── Acknowledge ─────────│
```

## Horizontal Scaling

### Zone-Based Scaling

```
                    ┌──────────────────┐
                    │     Gateway      │
                    │  (Load Balanced) │
                    └────────┬─────────┘
                             │
          ┌──────────────────┼──────────────────┐
          │                  │                  │
    ┌─────▼─────┐      ┌─────▼─────┐      ┌─────▼─────┐
    │  Zone 1   │      │  Zone 2   │      │  Zone 3   │
    │ (Starter) │      │ (Forest)  │      │ (Dungeon) │
    │  500 max  │      │  300 max  │      │  200 max  │
    └───────────┘      └───────────┘      └───────────┘
```

### Role-Based Scaling

```
Multiple Gateways for high connection count
Multiple Zones for game world distribution
Multiple Workers for background processing
```

## Configuration

```toml
# Distributed mode configuration
[distributed]
enabled = true
node_id = "zone-1"
node_type = "zone"

[distributed.network]
listen_address = "0.0.0.0:7780"
max_nodes = 10
heartbeat_interval = 5
node_timeout = 30

[distributed.state]
sync_method = "delta"  # full | delta
sync_interval = 1
max_sync_queue = 1000

[distributed.transfer]
enabled = true
timeout = 10
max_concurrent = 10
```

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Node communication | Same protocol as client-server | Simplicity, one codebase |
| State transfer | Serialized snapshots | Simple, predictable |
| Discovery | DNS + static fallback | Works everywhere |
| Scaling | Zone-based | Natural game world partitioning |
| Transfer | Gateway-mediated | Single source of truth for routing |

## References

- [10-server-mode.md](10-server-mode.md) - Server mode
- [11-gateway.md](11-gateway.md) - Gateway mode
- [07-protocol.md](07-protocol.md) - Protocol (shared between nodes)
