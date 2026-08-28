# 07-03 - 배포 및 CI/CD 설계

## 개요

The Protocol은 자동화된 CI/CD 파이프라인과 다중 배포 전략을 사용하여 안정적인 배포를 보장한다. GitHub Actions을 기반으로 하며, Docker와 Kubernetes 배포를 지원한다.

## CI/CD 파이프라인 (GitHub Actions)

### 파이프라인 흐름

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│   Lint   │───▶│  Test    │───▶│  Build   │───▶│  WASM    │───▶│ Release  │
│ (clippy) │    │(cargo)   │    │(Release) │    │ (Plugin) │    │ (Deploy) │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
     │               │               │               │               │
     ▼               ▼               ▼               ▼               ▼
  코드 품질       테스트 통과      바이너리 빌드    WASM 빌드     배포/릴리즈
```

### GitHub Actions 워크플로우

```yaml
# .github/workflows/ci.yml
name: CI/CD Pipeline

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  lint:
    name: Lint (Clippy)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - name: Check formatting
        run: cargo fmt --all -- --check
      - name: Run Clippy
        run: cargo clippy --workspace --all-targets --all-features -- -D warnings

  test:
    name: Test
    needs: lint
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_USER: postgres
          POSTGRES_PASSWORD: test
          POSTGRES_DB: the_protocol_test
        ports:
          - 5432:5432
        options: >-
          --health-cmd pg_isready
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
      redis:
        image: redis:7
        ports:
          - 6379:6379
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run tests
        env:
          DATABASE_URL: postgres://postgres:test@localhost:5432/the_protocol_test
          REDIS_URL: redis://localhost:6379
        run: cargo test --workspace --verbose
      - name: Generate coverage
        run: cargo tarpaulin --workspace --out Xml

  build:
    name: Build
    needs: test
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: the-protocol.exe
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: the-protocol
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
      - name: Build release
        run: cargo build --release --target ${{ matrix.target }}
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: target/${{ matrix.target }}/release/${{ matrix.artifact }}

  build-wasm:
    name: Build WASM Plugins
    needs: test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-wasip1
      - name: Build WASM plugins
        run: |
          cd plugins/character
          cargo build --release --target wasm32-wasip1
      - name: Upload WASM artifact
        uses: actions/upload-artifact@v4
        with:
          name: character.wasm
          path: plugins/character/target/wasm32-wasip1/release/character.wasm

  release:
    name: Release
    needs: [build, build-wasm]
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main'
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
      - name: Create Release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            the-protocol.exe
            the-protocol
            character.wasm
          generate_release_notes: true
```

## Docker 배포

### Dockerfile

```dockerfile
# multi-stage build
FROM rust:1.75 as builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin runtime

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/runtime /usr/local/bin/the-protocol

EXPOSE 7770 7771 8080

ENTRYPOINT ["the-protocol"]
CMD ["server", "--bind", "0.0.0.0:7770"]
```

### docker-compose.yml

```yaml
version: '3.8'

services:
  # 게임 서버
  game-server:
    build:
      context: .
      dockerfile: Dockerfile
    command: server --bind 0.0.0.0:7770 --plugins /plugins
    ports:
      - "7770:7770"   # TCP
      - "7771:7771"   # UDP
      - "8080:8080"   # HTTP API
    volumes:
      - ./plugins:/plugins
      - game-data:/data
    environment:
      - RUST_LOG=info
      - DATABASE_URL=postgres://postgres:password@postgres:5432/the_protocol
      - REDIS_URL=redis://redis:6379
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
    restart: unless-stopped
    networks:
      - game-network

  # 게이트웨이 (선택적)
  gateway:
    build:
      context: .
      dockerfile: Dockerfile
    command: gateway --bind 0.0.0.0:7772
    ports:
      - "7772:7772"
    environment:
      - RUST_LOG=info
    depends_on:
      - game-server
    restart: unless-stopped
    networks:
      - game-network

  # PostgreSQL
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: password
      POSTGRES_DB: the_protocol
    ports:
      - "5432:5432"
    volumes:
      - postgres-data:/var/lib/postgresql/data
      - ./migrations:/docker-entrypoint-initdb.d
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 10s
      timeout: 5s
      retries: 5
    restart: unless-stopped
    networks:
      - game-network

  # Redis
  redis:
    image: redis:7-alpine
    command: redis-server --appendonly yes
    ports:
      - "6379:6379"
    volumes:
      - redis-data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5
    restart: unless-stopped
    networks:
      - game-network

  # Prometheus (모니터링)
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml
    networks:
      - game-network

  # Grafana (대시보드)
  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - grafana-data:/var/lib/grafana
    networks:
      - game-network

volumes:
  game-data:
  postgres-data:
  redis-data:
  grafana-data:

networks:
  game-network:
    driver: bridge
```

### 서비스 분리

```
┌──────────────────────────────────────────────────────────┐
│                    Docker Network                        │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Game Server │  │   Gateway    │  │  HTTP API    │  │
│  │   :7770 TCP  │  │   :7772 TCP  │  │   :8080      │  │
│  │   :7771 UDP  │  │              │  │              │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         └─────────────────┼─────────────────┘           │
│                           │                              │
│                  ┌────────▼────────┐                     │
│                  │    PostgreSQL   │                     │
│                  │      :5432      │                     │
│                  └─────────────────┘                     │
│                                                          │
│                  ┌─────────────────┐                     │
│                  │     Redis       │                     │
│                  │      :6379      │                     │
│                  └─────────────────┘                     │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐                     │
│  │  Prometheus  │  │   Grafana    │                     │
│  │    :9090     │  │    :3000     │                     │
│  └──────────────┘  └──────────────┘                     │
└──────────────────────────────────────────────────────────┘
```

## Kubernetes 배포

### Deployment

```yaml
# k8s/game-server-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: game-server
  labels:
    app: the-protocol
    component: game-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: the-protocol
      component: game-server
  template:
    metadata:
      labels:
        app: the-protocol
        component: game-server
    spec:
      containers:
        - name: game-server
          image: the-protocol:latest
          args: ["server", "--bind", "0.0.0.0:7770", "--plugins", "/plugins"]
          ports:
            - containerPort: 7770
              name: tcp
              protocol: TCP
            - containerPort: 7771
              name: udp
              protocol: UDP
            - containerPort: 8080
              name: http
              protocol: TCP
          env:
            - name: RUST_LOG
              value: "info"
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: game-secrets
                  key: database-url
            - name: REDIS_URL
              valueFrom:
                secretKeyRef:
                  name: game-secrets
                  key: redis-url
          resources:
            requests:
              memory: "256Mi"
              cpu: "250m"
            limits:
              memory: "512Mi"
              cpu: "500m"
          livenessProbe:
            tcpSocket:
              port: 7770
            initialDelaySeconds: 10
            periodSeconds: 30
          readinessProbe:
            tcpSocket:
              port: 7770
            initialDelaySeconds: 5
            periodSeconds: 10
```

### Service

```yaml
# k8s/game-server-service.yaml
apiVersion: v1
kind: Service
metadata:
  name: game-server
spec:
  selector:
    app: the-protocol
    component: game-server
  ports:
    - name: tcp
      port: 7770
      targetPort: 7770
      protocol: TCP
    - name: udp
      port: 7771
      targetPort: 7771
      protocol: UDP
    - name: http
      port: 8080
      targetPort: 8080
      protocol: TCP
  type: LoadBalancer
```

### ConfigMap

```yaml
# k8s/configmap.yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: game-config
data:
  RUST_LOG: "info"
  DB_HOST: "postgres-service"
  DB_PORT: "5432"
  DB_NAME: "the_protocol"
  REDIS_HOST: "redis-service"
  REDIS_PORT: "6379"
  MAX_CONNECTIONS: "1000"
  PLUGIN_DIR: "/plugins"
```

### Horizontal Pod Autoscaler

```yaml
# k8s/hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: game-server-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: game-server
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
  behavior:
    scaleUp:
      stabilizationWindowSeconds: 60
      policies:
        - type: Pods
          value: 2
          periodSeconds: 60
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
        - type: Pods
          value: 1
          periodSeconds: 120
```

## 배포 레이아웃

### 개발 환경

```
┌─────────────────────────────────┐
│       Local Docker Compose      │
│                                 │
│  Game Server (1)                │
│  PostgreSQL (1)                 │
│  Redis (1)                      │
│  Prometheus + Grafana           │
└─────────────────────────────────┘
```

### 스테이징 환경

```
┌─────────────────────────────────┐
│       Kubernetes (Minikube)     │
│                                 │
│  Game Server (2 replicas)       │
│  PostgreSQL (1 replica)         │
│  Redis (1 replica)              │
│  Prometheus + Grafana           │
└─────────────────────────────────┘
```

### 프로덕션 환경

```
┌─────────────────────────────────┐
│    Kubernetes (Cloud Cluster)   │
│                                 │
│  Game Server (3~10 replicas)    │
│  Gateway (2 replicas)           │
│  PostgreSQL (HA: 3 replicas)    │
│  Redis Cluster (6 nodes)        │
│  Prometheus + Grafana           │
│  Log Aggregation (Loki)         │
│  Distributed Tracing (Jaeger)   │
└─────────────────────────────────┘
```

## 모니터링 (Prometheus, Grafana)

### Prometheus 메트릭

```rust
use prometheus::{
    Registry, IntCounter, IntGauge, Histogram,
    opts, histogram_opts,
};

pub struct Metrics {
    pub connections_total: IntCounter,
    pub active_connections: IntGauge,
    pub messages_received: IntCounter,
    pub messages_sent: IntCounter,
    pub command_duration: Histogram,
    pub plugin_load_time: Histogram,
    pub error_count: IntCounter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        let connections_total = IntCounter::with_opts(
            opts!("the_protocol_connections_total", "Total connections")
        ).unwrap();

        let active_connections = IntGauge::with_opts(
            opts!("the_protocol_active_connections", "Active connections")
        ).unwrap();

        let messages_received = IntCounter::with_opts(
            opts!("the_protocol_messages_received", "Messages received")
        ).unwrap();

        let messages_sent = IntCounter::with_opts(
            opts!("the_protocol_messages_sent", "Messages sent")
        ).unwrap();

        let command_duration = Histogram::with_opts(
            histogram_opts!("the_protocol_command_duration_seconds", "Command duration")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0])
        ).unwrap();

        let plugin_load_time = Histogram::with_opts(
            histogram_opts!("the_protocol_plugin_load_seconds", "Plugin load time")
        ).unwrap();

        let error_count = IntCounter::with_opts(
            opts!("the_protocol_errors_total", "Total errors")
        ).unwrap();

        registry.register(Box::new(connections_total.clone())).unwrap();
        registry.register(Box::new(active_connections.clone())).unwrap();
        registry.register(Box::new(messages_received.clone())).unwrap();
        registry.register(Box::new(messages_sent.clone())).unwrap();
        registry.register(Box::new(command_duration.clone())).unwrap();
        registry.register(Box::new(plugin_load_time.clone())).unwrap();
        registry.register(Box::new(error_count.clone())).unwrap();

        Self {
            connections_total,
            active_connections,
            messages_received,
            messages_sent,
            command_duration,
            plugin_load_time,
            error_count,
        }
    }
}
```

### Prometheus 설정

```yaml
# monitoring/prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'the-protocol'
    static_configs:
      - targets: ['game-server:8080']
    metrics_path: '/metrics'
```

### Grafana 대시보드

```
메인 대시보드:
├── 연결 수 (active_connections)
├── 메시지 처리량 (messages_received/sent)
├── 커맨드 응답 시간 (command_duration)
├── 에러율 (error_count)
├── 플러그인 로드 시간 (plugin_load_time)
└── 시스템 리소스 (CPU, Memory, Network)
```
