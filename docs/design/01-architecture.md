# 01 - Architecture

## Overview

The Protocol is a **Cross-Platform Game Runtime** written in Rust. It is NOT a game server - it is a programmable runtime that can assume different roles (Server, Client, Gateway, Peer, Tool) based on configuration and activated capabilities.

## Core Principle

> **Server is a role, not the identity of the Runtime.**

```
Runtime ≠ Server
Runtime + Server Capability = Server
Runtime + Client Capability = Client
Runtime + Gateway Capability = Gateway
Runtime + Client + Terminal UI = MUD Client
```

## High-Level Architecture

```
                         Cross-Platform Runtime (Rust)
                                  |
             ┌────────────────────┼────────────────────┐
             │                    │                    │
          Server                Client              Gateway
          Capability            Capability          Capability
             │                    │                    │
          TCP/UDP              TCP/UDP              TCP/UDP
          HTTP                 HTTP                 Routing
          WebSocket            WebSocket            Auth
             │                    │                    │
             └────────────────────┼────────────────────┘
                                  │
                            Plugin Runtime (WASM)
                                  │
                    ┌─────────────┼─────────────┐
                    ▼             ▼             ▼
                 Combat        Auction        Guild
                  WASM          WASM           WASM
```

## Layer Architecture

```
┌─────────────────────────────────────────────────┐
│                  CLI / Entry Point               │
├─────────────────────────────────────────────────┤
│               Runtime Core                       │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐│
│  │ Config   │ │ Scheduler│ │ Capability Manager││
│  └──────────┘ └──────────┘ └──────────────────┘│
├─────────────────────────────────────────────────┤
│              Network Layer                       │
│  ┌──────┐ ┌──────┐ ┌───────┐ ┌───────────────┐│
│  │ TCP  │ │ UDP  │ │ HTTP  │ │  WebSocket    ││
│  └──────┘ └──────┘ └───────┘ └───────────────┘│
├─────────────────────────────────────────────────┤
│              Protocol Layer                      │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐│
│  │ Codec    │ │ Routing  │ │ Serialization    ││
│  └──────────┘ └──────────┘ └──────────────────┘│
├─────────────────────────────────────────────────┤
│              Session Layer                       │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐│
│  │ Session  │ │ Auth     │ │ State            ││
│  └──────────┘ └──────────┘ └──────────────────┘│
├─────────────────────────────────────────────────┤
│              Plugin Runtime (WASM)               │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐│
│  │ Loader   │ │ Sandbox  │ │ Host Functions   ││
│  └──────────┘ └──────────┘ └──────────────────┘│
├─────────────────────────────────────────────────┤
│              Application Layer                   │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐│
│  │ Services │ │ Command  │ │ Event Bus        ││
│  └──────────┘ └──────────┘ └──────────────────┘│
├─────────────────────────────────────────────────┤
│              Domain Layer                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐│
│  │Character │ │ Combat   │ │ Inventory        ││
│  │ World    │ │ Auction  │ │ Guild            ││
│  └──────────┘ └──────────┘ └──────────────────┘│
└─────────────────────────────────────────────────┘
```

## Module Structure

```
/core
    /runtime        - Core runtime orchestration
    /network        - TCP, UDP, WebSocket transport
    /protocol       - Codec, message framing, serialization
    /session        - Connection session management
    /plugin         - WASM plugin loader and host
    /scheduler      - Task and timer scheduling
    /security       - Authentication, authorization, capability
    /routing        - Message routing, command dispatch
    /observability  - Logging, metrics, tracing

/domain            - Game domain entities (no infra dependencies)
/application       - Application services (combines domain logic)

/plugins           - WASM plugin implementations
    /character      - Character creation, management
    /combat         - Combat system
    /inventory      - Inventory management
    /auction        - Auction house

/sdk               - Plugin SDKs for external developers
    /typescript     - TypeScript SDK
    /csharp         - C# SDK

/clients           - Client applications
    /mud            - MUD terminal client

/api               - HTTP/Web API (optional separate deployment)
/tools             - Development and admin tools
/tests             - Integration and acceptance tests
```

## Technology Choices

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Performance, safety, WASM support |
| Async Runtime | Tokio | Industry standard, mature ecosystem |
| TCP/UDP | tokio + bytes | Async networking |
| HTTP | Axum | Modern, Tokio-native, lightweight |
| WebSocket | tokio-tungstenite | Mature, async-native |
| WASM Runtime | Wasmtime | Best embedding API, safety, ByteCode Alliance |
| Serialization | MessagePack (rmp-serde) | Compact, fast, schema-flexible |
| Database | PostgreSQL (sqlx) | Async, type-safe, proven |
| Cache | Redis (fred) | Async, feature-rich |
| Logging | tracing | Structured, async-native |
| Metrics | metrics + metrics-exporter | Pluggable exporters |
| CLI | clap | Declarative, derive-based |

## Design Decisions

### Why not "Server"?
Games are not just client-server. A game runtime needs to be:
- A server hosting game worlds
- A client connecting to worlds
- A gateway routing traffic
- A peer in distributed setups
- A tool for administration

Naming it "Server" limits mental model and architecture.

### Why WASM for plugins?
- Platform-independent: same `.wasm` on Windows and Linux
- Sandboxed: memory safety, no arbitrary system access
- Language-agnostic: Rust, C, C++, AssemblyScript, TinyGo compile to WASM
- Hot-reloadable: replace WASM instances without restarting
- Verifiable: capability-based security model

### Why MessagePack over Protobuf?
- No code generation required (optional)
- Schema evolution is simpler
- Compact binary format
- Excellent Rust and TypeScript support
- Lower barrier for plugin developers

### Why not a monolithic server?
- Different roles need different resource profiles
- Gateway needs low memory, high connection count
- Game zones need high CPU, moderate connections
- Horizontal scaling requires independent processes
- Development speed: one codebase, multiple deployables

## Cross-Platform Build

```
Source Code (single Rust codebase)
        │
        ├── CI/CD ──→ Windows x64 ──→ runtime.exe
        │
        └── CI/CD ──→ Linux x64 ──→ runtime

Plugins (platform-independent)
        │
        └── WASM build ──→ *.wasm (shared across platforms)
```

Deployment layout:
```
release/
    windows-x64/
        runtime.exe
        config.toml
        plugins/
            combat.wasm
            inventory.wasm
            character.wasm
            auction.wasm
    linux-x64/
        runtime
        config.toml
        plugins/
            combat.wasm
            inventory.wasm
            character.wasm
            auction.wasm
```

## Data Flow

```
Client Command
      │
      ▼
  Transport (TCP/UDP/WS)
      │
      ▼
  Protocol Decode
      │
      ▼
  Session Lookup
      │
      ▼
  Command Router
      │
      ├──→ Plugin Handler (WASM)
      │         │
      │         ▼
      │    Domain Logic
      │         │
      │         ▼
      │    Event Emitted
      │         │
      │         ▼
      │    Event Bus
      │         │
      └──→ Response/Event
                │
                ▼
           Protocol Encode
                │
                ▼
           Transport Send
```

## References

- [02-runtime.md](02-runtime.md) - Core Runtime design
- [03-capability.md](03-capability.md) - Capability system
- [04-plugin-system.md](04-plugin-system.md) - Plugin lifecycle
- [07-protocol.md](07-protocol.md) - Network protocol
- [08-network.md](08-network.md) - Network layer
