# 08 - Network Layer

## Overview

The Network Layer manages TCP, UDP, HTTP, and WebSocket connections. It is transport-agnostic from the Protocol layer's perspective.

## Network Architecture

```
┌─────────────────────────────────────────────┐
│              Network Manager                 │
│                                             │
│  ┌─────────┐ ┌─────────┐ ┌───────────────┐│
│  │   TCP   │ │   UDP   │ │  HTTP/Axum    ││
│  │ Listener│ │ Listener│ │  Server       ││
│  └────┬────┘ └────┬────┘ └──────┬────────┘│
│       │           │              │          │
│  ┌────▼───────────▼──────────────▼────────┐│
│  │         Protocol Codec                  ││
│  │    (encode/decode Messages)            ││
│  └────────────────┬───────────────────────┘│
│                   │                         │
│  ┌────────────────▼───────────────────────┐│
│  │         Session Manager                ││
│  │    (track connections, routing)        ││
│  └────────────────────────────────────────┘│
└─────────────────────────────────────────────┘
```

## TCP Listener

```rust
pub struct TcpListener {
    listener: tokio::net::TcpListener,
    codec: ProtocolCodec,
    session_manager: Arc<SessionManager>,
    config: TcpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpConfig {
    pub bind_address: String,
    pub max_connections: usize,
    pub nodelay: bool,
    pub keepalive: Option<Duration>,
    pub read_timeout: Option<Duration>,
    pub write_timeout: Option<Duration>,
}

impl TcpListener {
    pub async fn new(config: TcpConfig, session_manager: Arc<SessionManager>) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
        tracing::info!("TCP listening on {}", config.bind_address);

        Ok(Self {
            listener,
            codec: ProtocolCodec::new(),
            session_manager,
            config,
        })
    }

    pub async fn accept_loop(&self) -> Result<()> {
        loop {
            let (socket, addr) = self.listener.accept().await?;

            // Check connection limit
            if self.session_manager.count() >= self.config.max_connections {
                tracing::warn!("Connection limit reached, rejecting {}", addr);
                socket.shutdown(Shutdown::Both)?;
                continue;
            }

            // Apply socket options
            socket.set_nodelay(self.config.nodelay)?;

            let session_manager = self.session_manager.clone();
            let codec = self.codec.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_connection(socket, addr, codec, session_manager).await {
                    tracing::error!("Connection error from {}: {}", addr, e);
                }
            });
        }
    }

    async fn handle_connection(
        socket: TcpStream,
        addr: SocketAddr,
        codec: ProtocolCodec,
        session_manager: Arc<SessionManager>,
    ) -> Result<()> {
        let session_id = session_manager.create_tcp_session(addr).await?;
        let mut framed = TcpFramed::new(socket, codec);

        // Handshake
        let hello: Message = framed.read_frame().await?;
        // ... validate Hello, create session ...

        let session = session_manager.get(session_id).await?;

        loop {
            tokio::select! {
                msg = framed.read_frame() => {
                    match msg {
                        Ok(message) => {
                            session_manager.route_message(session_id, message).await?;
                        }
                        Err(_) => break,
                    }
                }
                msg = session.recv_outgoing() => {
                    if let Some(message) = msg {
                        framed.write_frame(&message).await?;
                    }
                }
            }
        }

        session_manager.remove(session_id).await;
        Ok(())
    }
}
```

## UDP Listener

```rust
pub struct UdpListener {
    socket: Arc<UdpSocket>,
    codec: ProtocolCodec,
    session_manager: Arc<SessionManager>,
    config: UdpConfig,
    peers: HashMap<SocketAddr, UdpPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpConfig {
    pub bind_address: String,
    pub max_peers: usize,
    pub buffer_size: usize,
    pub heartbeat_timeout: Duration,
}

pub struct UdpPeer {
    pub addr: SocketAddr,
    pub session_id: u64,
    pub last_seen: Instant,
    pub sequence: u32,
    pub ack: u32,
}

impl UdpListener {
    pub async fn new(config: UdpConfig, session_manager: Arc<SessionManager>) -> Result<Self> {
        let socket = Arc::new(UdpSocket::bind(&config.bind_address).await?);
        tracing::info!("UDP listening on {}", config.bind_address);

        Ok(Self {
            socket,
            codec: ProtocolCodec::new(),
            session_manager,
            config,
            peers: HashMap::new(),
        })
    }

    pub async fn recv_loop(&mut self) -> Result<()> {
        let mut buf = vec![0u8; self.config.buffer_size];

        loop {
            let (len, addr) = self.socket.recv_from(&mut buf).await?;

            if let Some(peer) = self.peers.get_mut(&addr) {
                // Known peer
                peer.last_seen = Instant::now();
                let message = self.codec.decode(&mut BytesMut::from(&buf[..len]))?;
                if let Some(msg) = message {
                    self.session_manager.route_message(peer.session_id, msg).await?;
                }
            } else {
                // New peer - create session
                let session_id = self.session_manager.create_udp_session(addr).await?;
                self.peers.insert(addr, UdpPeer {
                    addr,
                    session_id,
                    last_seen: Instant::now(),
                    sequence: 0,
                    ack: 0,
                });
            }
        }
    }

    pub async fn send_to(&self, addr: SocketAddr, message: &Message) -> Result<()> {
        let data = self.codec.encode(message)?;
        self.socket.send_to(&data, addr).await?;
        Ok(())
    }
}
```

## HTTP Server (Axum)

```rust
pub struct HttpServer {
    router: Router,
    config: HttpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    pub bind_address: String,
    pub cors_origins: Vec<String>,
    pub max_request_size: usize,
}

impl HttpServer {
    pub fn new(config: HttpConfig, plugin_runtime: Arc<PluginRuntime>) -> Self {
        let router = Router::new()
            .route("/health", get(health_check))
            .route("/status", get(status))
            .route("/api/v1/:path*", any(api_handler))
            .layer(CorsLayer::permissive())
            .with_state(plugin_runtime);

        Self { router, config }
    }

    pub async fn start(&self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.config.bind_address).await?;
        tracing::info!("HTTP listening on {}", self.config.bind_address);
        axum::serve(listener, self.router.clone()).await?;
        Ok(())
    }
}

async fn health_check() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn api_handler(
    State(plugin_runtime): State<Arc<PluginRuntime>>,
    Path(path): Path<String>,
    method: Method,
    body: Bytes,
) -> impl IntoResponse {
    // Route to plugin HTTP handlers
    match plugin_runtime.handle_http(&method, &path, &body).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}
```

## WebSocket Transport

```rust
pub struct WebSocketListener {
    listener: tokio::net::TcpListener,
    codec: ProtocolCodec,
    session_manager: Arc<SessionManager>,
}

impl WebSocketListener {
    pub async fn new(config: WebSocketConfig, session_manager: Arc<SessionManager>) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
        tracing::info!("WebSocket listening on {}", config.bind_address);

        Ok(Self {
            listener,
            codec: ProtocolCodec::new(),
            session_manager,
        })
    }

    pub async fn accept_loop(&self) -> Result<()> {
        loop {
            let (socket, addr) = self.listener.accept().await?;
            let ws = accept_async(socket).await?;
            let session_manager = self.session_manager.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_websocket(ws, addr, session_manager).await {
                    tracing::error!("WebSocket error from {}: {}", addr, e);
                }
            });
        }
    }

    async fn handle_websocket(
        ws: WebSocket<TcpStream>,
        addr: SocketAddr,
        session_manager: Arc<SessionManager>,
    ) -> Result<()> {
        let (mut ws_sender, mut ws_receiver) = ws.split();
        let session_id = session_manager.create_ws_session(addr).await?;
        let session = session_manager.get(session_id).await?;

        loop {
            tokio::select! {
                Some(msg) = ws_receiver.next() => {
                    match msg? {
                        Message::Binary(data) => {
                            let protocol_msg = rmp_serde::from_slice(&data)?;
                            session_manager.route_message(session_id, protocol_msg).await?;
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
                Some(msg) = session.recv_outgoing() => {
                    let data = rmp_serde::to_vec(&msg)?;
                    ws_sender.send(Message::Binary(data)).await?;
                }
            }
        }

        session_manager.remove(session_id).await;
        Ok(())
    }
}
```

## Network Manager

```rust
pub struct NetworkManager {
    tcp_listener: Option<TcpListener>,
    udp_listener: Option<UdpListener>,
    http_server: Option<HttpServer>,
    ws_listener: Option<WebSocketListener>,
    session_manager: Arc<SessionManager>,
}

impl NetworkManager {
    pub async fn new(config: &NetworkConfig, session_manager: Arc<SessionManager>) -> Result<Self> {
        let mut manager = Self {
            tcp_listener: None,
            udp_listener: None,
            http_server: None,
            ws_listener: None,
            session_manager,
        };

        if config.tcp.enabled {
            manager.tcp_listener = Some(TcpListener::new(config.tcp.clone(), manager.session_manager.clone()).await?);
        }

        if config.udp.enabled {
            manager.udp_listener = Some(UdpListener::new(config.udp.clone(), manager.session_manager.clone()).await?);
        }

        if config.http.enabled {
            manager.http_server = Some(HttpServer::new(config.http.clone(), ...));
        }

        if config.websocket.enabled {
            manager.ws_listener = Some(WebSocketListener::new(config.websocket.clone(), manager.session_manager.clone()).await?);
        }

        Ok(manager)
    }

    pub async fn start(&self) -> Result<()> {
        let mut handles = Vec::new();

        if let Some(ref tcp) = self.tcp_listener {
            handles.push(tokio::spawn(async move { tcp.accept_loop().await }));
        }
        if let Some(ref udp) = self.udp_listener {
            handles.push(tokio::spawn(async move { udp.recv_loop().await }));
        }
        if let Some(ref http) = self.http_server {
            handles.push(tokio::spawn(async move { http.start().await }));
        }
        if let Some(ref ws) = self.ws_listener {
            handles.push(tokio::spawn(async move { ws.accept_loop().await }));
        }

        // All listeners run concurrently
        futures::future::join_all(handles).await;
        Ok(())
    }
}
```

## Connection Pool (Client Mode)

```rust
pub struct ConnectionPool {
    connections: HashMap<String, PooledConnection>,
    config: PoolConfig,
}

pub struct PooledConnection {
    pub id: String,
    pub transport: Transport,
    pub state: ConnectionState,
    pub last_activity: Instant,
}

#[derive(Debug)]
pub enum Transport {
    Tcp(TcpFramed),
    Udp(UdpSocket),
    WebSocket(WebSocket<TcpStream>),
}

impl ConnectionPool {
    pub async fn connect(&mut self, address: &str, transport_type: TransportType) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let transport = match transport_type {
            TransportType::Tcp => Transport::Tcp(TcpFramed::connect(address).await?),
            TransportType::Udp => Transport::Udp(UdpSocket::bind("0.0.0.0:0").await?),
            TransportType::WebSocket => {
                let ws = connect_async(address).await?;
                Transport::WebSocket(ws)
            }
        };

        self.connections.insert(id.clone(), PooledConnection {
            id: id.clone(),
            transport,
            state: ConnectionState::Connected,
            last_activity: Instant::now(),
        });

        Ok(id)
    }
}
```

## Config

```toml
[network]
max_connections = 10000

[network.tcp]
enabled = true
bind_address = "0.0.0.0:7770"
nodelay = true
keepalive = 30

[network.udp]
enabled = true
bind_address = "0.0.0.0:7771"
buffer_size = 65535
heartbeat_timeout = 60

[network.http]
enabled = true
bind_address = "0.0.0.0:8080"
max_request_size = 1048576

[network.websocket]
enabled = true
bind_address = "0.0.0.0:8081"
```

## References

- [07-protocol.md](07-protocol.md) - Message format and codec
- [09-client.md](09-client.md) - Client mode networking
- [10-server-mode.md](10-server-mode.md) - Server mode networking
