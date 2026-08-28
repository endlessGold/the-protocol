# 17 - Deployment

## Overview

The Runtime produces platform-native binaries for Windows and Linux, while WASM plugins are platform-independent and shared across all deployments.

## Build Targets

| Artifact | Platform | Binary |
|----------|----------|--------|
| Runtime | Windows x64 | `runtime.exe` |
| Runtime | Linux x64 | `runtime` |
| Runtime | Linux ARM64 | `runtime` (cross-compiled) |
| Runtime | Windows ARM64 | `runtime.exe` (cross-compiled) |
| Plugins | Platform-independent | `*.wasm` |

## CI/CD Pipeline

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Lint      │───→│   Test      │───→│   Build     │
│   (clippy)  │    │   (cargo)   │    │             │
└─────────────┘    └─────────────┘    └──────┬──────┘
                                              │
                    ┌─────────────────────────┼─────────────────────────┐
                    │                         │                         │
              ┌─────▼─────┐           ┌───────▼─────┐           ┌──────▼──────┐
              │ Windows   │           │  Linux      │           │  WASM       │
              │ x64 build │           │  x64 build  │           │  Plugins    │
              └─────┬─────┘           └───────┬─────┘           └──────┬──────┘
                    │                         │                         │
                    ▼                         ▼                         ▼
              runtime.exe               runtime                *.wasm files
```

### GitHub Actions

```yaml
name: Build and Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            artifact: runtime.exe
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            artifact: runtime

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.target }}
          path: target/${{ matrix.target }}/release/${{ matrix.artifact }}

  build-plugins:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-wasi

      - name: Build plugins
        run: |
          for plugin in plugins/*/; do
            cargo build --release --target wasm32-wasi --manifest-path "${plugin}Cargo.toml"
          done

      - name: Upload plugins
        uses: actions/upload-artifact@v4
        with:
          name: plugins
          path: plugins/*/target/wasm32-wasi/release/*.wasm

  release:
    needs: [build, build-plugins]
    runs-on: ubuntu-latest
    steps:
      - name: Download artifacts
        uses: actions/download-artifact@v4

      - name: Create release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            x86_64-pc-windows-msvc/runtime.exe
            x86_64-unknown-linux-gnu/runtime
            plugins/**/*.wasm
```

## Release Layout

```
release/
    windows-x64/
        runtime.exe
        config.toml
        plugins/
            character.wasm
            combat.wasm
            inventory.wasm
            auction.wasm
        data/
            worlds/
                default.toml

    linux-x64/
        runtime
        config.toml
        plugins/
            character.wasm
            combat.wasm
            inventory.wasm
            auction.wasm
        data/
            worlds/
                default.toml
```

## Docker Deployment

```dockerfile
# Build stage
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-gnu

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/runtime .
COPY --from=builder /app/plugins/*/target/wasm32-wasi/release/*.wasm ./plugins/
COPY config.toml .

EXPOSE 7770 7771 8080 8081

CMD ["./runtime", "server"]
```

```yaml
# docker-compose.yml
version: '3.8'

services:
  gateway:
    build: .
    command: ["./runtime", "gateway"]
    ports:
      - "7770:7770"
      - "7771:7771"
      - "8080:8080"
    environment:
      - JWT_SECRET=${JWT_SECRET}
      - DATABASE_URL=postgresql://user:pass@postgres:5432/the_protocol
      - REDIS_URL=redis://redis:6379
    depends_on:
      - postgres
      - redis

  zone-1:
    build: .
    command: ["./runtime", "server", "--config", "zone-1.toml"]
    ports:
      - "7771:7770"
    environment:
      - DATABASE_URL=postgresql://user:pass@postgres:5432/the_protocol
      - REDIS_URL=redis://redis:6379
    depends_on:
      - postgres
      - redis
      - gateway

  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: user
      POSTGRES_PASSWORD: pass
      POSTGRES_DB: the_protocol
    volumes:
      - pgdata:/var/lib/postgresql/data
      - ./migrations:/docker-entrypoint-initdb.d

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"

volumes:
  pgdata:
```

## Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: game-runtime
spec:
  replicas: 3
  selector:
    matchLabels:
      app: game-runtime
  template:
    metadata:
      labels:
        app: game-runtime
    spec:
      containers:
        - name: runtime
          image: ghcr.io/the-protocol/runtime:latest
          args: ["server"]
          ports:
            - containerPort: 7770
              name: tcp
            - containerPort: 7771
              name: udp
            - containerPort: 8080
              name: http
          env:
            - name: JWT_SECRET
              valueFrom:
                secretKeyRef:
                  name: game-secrets
                  key: jwt-secret
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: game-secrets
                  key: database-url
          resources:
            limits:
              memory: "512Mi"
              cpu: "500m"
            requests:
              memory: "256Mi"
              cpu: "250m"
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 10
            periodSeconds: 30
          readinessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 10
---
apiVersion: v1
kind: Service
metadata:
  name: game-runtime
spec:
  selector:
    app: game-runtime
  ports:
    - name: tcp
      port: 7770
      targetPort: 7770
    - name: udp
      port: 7771
      targetPort: 7771
    - name: http
      port: 8080
      targetPort: 8080
  type: LoadBalancer
```

## Configuration Management

```toml
# config.toml - Environment-specific overrides

[runtime]
mode = "server"

[server]
bind_address = "0.0.0.0:7770"

[database.postgres]
url = "${DATABASE_URL}"

[database.redis]
url = "${REDIS_URL}"

[security]
jwt_secret = "${JWT_SECRET}"
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string | `postgresql://localhost:5432/the_protocol` |
| `REDIS_URL` | Redis connection string | `redis://localhost:6379` |
| `JWT_SECRET` | JWT signing secret | (required) |
| `RUST_LOG` | Log level filter | `info` |
| `RUNTIME_MODE` | Override runtime mode | (from config) |

## Monitoring

```toml
[observability]
[observability.metrics]
enabled = true
endpoint = "/metrics"
exporter = "prometheus"

[observability.logging]
level = "info"
format = "json"
output = "stdout"

[observability.tracing]
enabled = true
endpoint = "http://jaeger:14268/api/traces"
```

## Backup Strategy

| Component | Method | Frequency |
|-----------|--------|-----------|
| PostgreSQL | pg_dump | Daily |
| Redis | RDB snapshot | Every 6 hours |
| WASM Plugins | File backup | On deploy |
| Configuration | Git | On change |

## References

- [01-architecture.md](01-architecture.md) - Overall architecture
- [02-runtime.md](02-runtime.md) - Runtime design
- [06-wasm.md](06-wasm.md) - WASM plugin deployment
