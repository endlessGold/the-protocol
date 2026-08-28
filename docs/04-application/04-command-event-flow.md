# Command/Event 흐름 설계

---

## 1. 전체 Command/Event 흐름

### 1.1 흐름 다이어그램

```
┌──────────┐     ┌─────────────────────────────────────────────────┐     ┌──────────┐
│  Client  │     │              Server Runtime                      │     │  Client  │
│ (MUD)    │     │                                                  │     │ (Game)   │
└────┬─────┘     │                                                  └─────┴────┬─────┘
     │           │                                                           │
     │ 1. TCP 연결 establishing                                              │
     │──────────────────────────────────────────────────────────────────────▶│
     │           │                                                           │
     │           │  NetworkManager::accept_connections()                     │
     │           │  → TcpListener::accept()                                 │
     │           │  → SessionManager::create_session()                       │
     │           │                                                           │
     │ 2. Hello 메시지 (Handshake)                                          │
     │──────────────────────────────────────────────────────────────────────▶│
     │           │                                                           │
     │           │  Message::hello(ClientType::MUD, auth_token)              │
     │           │  → ProtocolCodec::decode_simple()                         │
     │           │  → MessageType::Hello                                     │
     │           │                                                           │
     │ 3. HelloAck 응답                                                     │
     │◀──────────────────────────────────────────────────────────────────────│
     │           │                                                           │
     │           │  HelloAck { session_id, server_time, capabilities }       │
     │           │  SessionState → Authenticated                             │
     │           │                                                           │
     │ 4. Command 전송 (look, move, attack 등)                               │
     │──────────────────────────────────────────────────────────────────────▶│
     │           │                                                           │
     │           │  ┌─────────────────────────────────────────────┐          │
     │           │  │ Network Layer                                │          │
     │           │  │ reader.read_exact() → ProtocolCodec::decode  │          │
     │           │  │ → Message { MessageType::Command }           │          │
     │           │  │ → session.send(message)                      │          │
     │           │  └──────────────────┬──────────────────────────┘          │
     │           │                     │                                     │
     │           │  ┌──────────────────▼──────────────────────────┐          │
     │           │  │ Session Layer                                │          │
     │           │  │ session.incoming_rx                          │          │
     │           │  │ → mpsc::channel으로 라우팅                   │          │
     │           │  └──────────────────┬──────────────────────────┘          │
     │           │                     │                                     │
     │           │  ┌──────────────────▼──────────────────────────┐          │
     │           │  │ Command Router                               │          │
     │           │  │ Command { command_type: "look" }             │          │
     │           │  │ → handlers.get("look")                       │          │
     │           │  │ → handler.handle(command, session_id)        │          │
     │           │  └──────────────────┬──────────────────────────┘          │
     │           │                     │                                     │
     │           │  ┌──────────────────▼──────────────────────────┐          │
     │           │  │ Command Handler (e.g. LookHandler)           │          │
     │           │  │ rmp_serde::from_slice(&command.payload)      │          │
     │           │  │ → GameWorld::look_room(room_id)              │          │
     │           │  │   → Domain Layer 호출                       │          │
     │           │  │   → DomainEvent 발생                        │          │
     │           │  │ → LookResponse 생성                          │          │
     │           │  │ → CommandResponse { success: true, payload } │          │
     │           │  └──────────────────┬──────────────────────────┘          │
     │           │                     │                                     │
     │           │  ┌──────────────────▼──────────────────────────┐          │
     │           │  │ Network Layer (응답 전송)                    │          │
     │           │  │ ProtocolCodec::encode(response)              │          │
     │           │  │ writer.write_all(&encoded)                   │          │
     │           │  └──────────────────┬──────────────────────────┘          │
     │           │                     │                                     │
     │ 5. CommandResponse 수신                                               │
     │◀──────────────────────────────────────────────────────────────────────│
     │           │                                                           │
     │ 6. 클라이언트에서 응답 파싱 및 표시                                    │
     │           │                                                           │
```

### 1.2 전체 흐름 요약

| 단계 | 레이어 | 설명 | 현재 구현 |
|------|--------|------|-----------|
| 1 | Network | TCP 연결 수립 | ✅ `NetworkManager::accept_connections()` |
| 2 | Network | Handshake (Hello) | ✅ `Message::hello()` |
| 3 | Session | 세션 생성 및 인증 | ✅ `SessionManager::create_session()` |
| 4 | Protocol | 메시지 디코딩 | ✅ `ProtocolCodec::decode_simple()` |
| 5 | Routing | 커맨드 라우팅 | ✅ `CommandRouter::route()` |
| 6 | Application | 서비스 로직 실행 | ✅ `GameWorld` 메서드 |
| 7 | Domain | 도메인 로직/상태 변경 | ✅ `Character`, `Combat` |
| 8 | Domain | 이벤트 발생 | ⚠️ 생성만 하고 미소비 |
| 9 | Protocol | 응답 인코딩 | ✅ `ProtocolCodec::encode()` |
| 10 | Network | 응답 전송 | ✅ `writer.write_all()` |

---

## 2. 각 단계별 상세 처리

### 2.1 단계 1: TCP 연결 수립

**진입점:** `core/network/src/lib.rs:53`

```rust
pub async fn accept_connections(&self) -> Result<(), NetworkError> {
    let listener = self.tcp_listener.as_ref().ok_or(NetworkError::Closed)?;

    loop {
        let (socket, addr) = listener.accept().await?;

        // 연결 제한 검사
        if !self.session_manager.can_accept() {
            tracing::warn!("Connection limit reached, rejecting {}", addr);
            drop(socket);
            continue;
        }

        socket.set_nodelay(true)?;  // TCP_NODELAY 설정 (게임에 필수)

        // 세션 매니저와 코덱을 클론하여 새 태스크에 전달
        let session_manager = self.session_manager.clone();
        let codec = self.codec.clone();

        tokio::spawn(async move {
            Self::handle_connection(socket, addr, codec, session_manager).await
        });
    }
}
```

**상세 처리:**
- `TcpListener::accept()`로 새 연결 수락
- `SessionManager::can_accept()`로 동시 연결 제한 검사 (현재: 1000개)
- `socket.set_nodelay(true)` — TCP Nagle 알고리즘 비활성화 (게임 응답 지연 방지)
- 각 연결마다 별도 `tokio::spawn`으로 독립 태스크 생성

### 2.2 단계 2-3: Handshake 및 세션 생성

**진입점:** `core/network/src/lib.rs:79`

```rust
async fn handle_connection(
    socket: TcpStream,
    addr: SocketAddr,
    codec: ProtocolCodec,
    session_manager: Arc<SessionManager>,
) -> Result<(), NetworkError> {
    // 세션 생성
    let session_id = session_manager.create_session(
        addr,
        protocol_session::TransportType::Tcp,
    )?;

    let (mut reader, mut writer) = socket.into_split();

    // Hello 메시지 수신
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let total_len = u32::from_be_bytes(len_buf) as usize;

    let mut frame = vec![0u8; total_len - 4];
    reader.read_exact(&mut frame).await?;

    // 디코딩
    let mut full_frame = BytesMut::with_capacity(4 + total_len);
    full_frame.put_slice(&len_buf);
    full_frame.put_slice(&frame);

    let mut buf = full_frame;
    let hello_msg = codec.decode_simple(&mut buf)?
        .ok_or(NetworkError::Closed)?;

    // HelloAck 응답
    let hello_ack = Message::hello_ack(session_id, vec!["game".to_string()]);
    let ack_bytes = codec.encode(&hello_ack)?;
    writer.write_all(&ack_bytes).await?;

    // 세션 상태 갱신
    if let Some(mut session) = session_manager.get(session_id) {
        session.set_state(protocol_session::SessionState::Authenticated);
    }

    // 메인 루프 진입
    // ...
}
```

**프레임 구조 (바이너리 프로토콜):**

```
┌──────────┬──────────┬──────────┬──────────┬──────────┬──────────┐
│ Length   │ Version  │ Msg ID   │ Msg Type │ Payload  │ CRC32    │
│ (4 bytes)│ (1 byte) │ (8 bytes)│ (1 byte) │ (N bytes)│ (4 bytes)│
│ u32 BE   │ u8       │ u64 BE   │ u8       │ Vec<u8>  │ u32 BE   │
└──────────┴──────────┴──────────┴──────────┴──────────┴──────────┘
```

- **Length**: 전체 프레임 길이 (Length 필드 포함)
- **Version**: 프로토콜 버전 (현재: 1)
- **Msg ID**: 메시지 고유 ID (난수)
- **Msg Type**: 메시지 타입 (`0x01`=Command, `0x02`=Response, `0x10`=Event 등)
- **Payload**: rmp-serde로 직렬화된 데이터
- **CRC32**: 페이로드 무결성 검증

### 2.3 단계 4: 메시지 디코딩

**진입점:** `core/protocol/src/codec.rs:93`

```rust
pub fn decode_simple(buf: &mut BytesMut) -> Result<Option<Message>, CodecError> {
    if buf.len() < 4 {
        return Ok(None);  // 프레임 불완전
    }

    let total_len = {
        let mut peek = buf.clone();
        peek.get_u32() as usize
    };

    if buf.len() < total_len {
        return Ok(None);  // 프레임 불완전 (더 많은 데이터 필요)
    }

    buf.get_u32();        // length
    let version = buf.get_u8();
    let id = buf.get_u64();
    let message_type_byte = buf.get_u8();

    let message_type = MessageType::from_u8(message_type_byte)
        .ok_or(CodecError::InvalidMessageType(message_type_byte))?;

    let payload_len = total_len - 14 - 4;  // 전체 - 헤더 - 체크섬
    let mut payload = vec![0u8; payload_len];
    buf.copy_to_slice(&mut payload);

    let _checksum = buf.get_u32();

    Ok(Some(Message { version, id, message_type, payload }))
}
```

**메시지 타입 매핑:**

| 타입 코드 | 이름 | 용도 |
|-----------|------|------|
| `0x01` | Command | 클라이언트 → 서버 커맨드 |
| `0x02` | CommandResponse | 서버 → 클라이언트 응답 |
| `0x10` | Event | 서버 → 클라이언트 이벤트 |
| `0x11` | EventAck | 이벤트 확인 |
| `0x20` | Ping | 연결 확인 |
| `0x21` | Pong | 연결 확인 응답 |
| `0x22` | Hello | 연결 초기화 |
| `0x23` | HelloAck | 연결 초기화 응답 |
| `0x24` | Disconnect | 연결 해제 |
| `0x25` | Error | 에러 응답 |
| `0x30` | PluginMessage | 플러그인 메시지 |
| `0x31` | PluginResponse | 플러그인 응답 |

### 2.4 단계 5: 세션 라우팅

**진입점:** `core/network/src/lib.rs:126`

```rust
// 메인 읽기 루프
loop {
    tokio::select! {
        result = reader.read_exact(&mut len_buf) => {
            match result {
                Ok(()) => {
                    // 프레임 읽기
                    let total_len = u32::from_be_bytes(len_buf) as usize;
                    let mut frame = vec![0u8; total_len - 4];
                    reader.read_exact(&mut frame).await?;

                    // 디코딩
                    let mut full_frame = BytesMut::with_capacity(4 + total_len);
                    full_frame.put_slice(&len_buf);
                    full_frame.put_slice(&frame);

                    let mut buf = full_frame;
                    if let Some(message) = codec.decode_simple(&mut buf)? {
                        // 세션의 incoming channel으로 전달
                        if let Some(session) = session_manager.get(session_id) {
                            let _ = session.send(message);
                        }
                    }
                }
                Err(e) => {
                    tracing::info!("Session {} disconnected: {}", session_id, e);
                    break;
                }
            }
        }
        // 나가는 메시지 처리 (서버 → 클라이언트)
        Some(outgoing) = {
            let mut rx = incoming_rx.lock().await;
            rx.recv().await
        } => {
            let encoded = codec.encode(&outgoing)?;
            if writer.write_all(&encoded).await.is_err() {
                break;
            }
        }
    }
}
```

**`tokio::select!` 동작:**
- **읽기 분기**: 클라이언트에서 새 데이터 수신 시 디코딩 후 세션의 `mpsc::channel`로 전달
- **쓰기 분기**: 세션의 outgoing channel에서 메시지 수신 시 인코딩 후 클라이언트에 전송
- 양쪽이 동시에 가능하여 풀덱스 동작

### 2.5 단계 6: 커맨드 라우터

**진입점:** `core/routing/src/lib.rs:39`

```rust
pub async fn route(
    &self,
    command: Command,
    session_id: u64,
) -> Result<CommandResponse, RoutingError> {
    let handler = self.handlers
        .get(&command.command_type)          // DashMap에서 핸들러 조회
        .ok_or_else(|| RoutingError::UnknownCommand(command.command_type.clone()))?;

    handler.handle(command, session_id).await  // 핸들러 실행
}
```

**등록된 커맨드 (현재):**

| 커맨드 | 핸들러 | 설명 |
|--------|--------|------|
| `"look"` | `LookHandler` | 방 정보 조회 |
| `"move"` | `MoveHandler` | 이동 |
| `"attack"` | `AttackHandler` | 전투 시작 |
| `"inventory"` | `InventoryHandler` | 인벤토리 조회 |
| `"create_character"` | `CreateCharacterHandler` | 캐릭터 생성 |

**커맨드 구조:**

```rust
pub struct Command {
    pub id: u64,              // 고유 ID
    pub command_type: String, // 커맨드 식별자
    pub session_id: u64,      // 클라이언트 세션 ID
    pub timestamp: u64,       // 타임스탬프
    pub payload: Vec<u8>,     // rmp-serde 직렬화된 파라미터
}
```

### 2.6 단계 7: 핸들러 처리 (상세)

#### LookHandler

```rust
struct LookHandler {
    game_world: Arc<RwLock<GameWorld>>,
}

impl CommandHandler for LookHandler {
    async fn handle(&self, _command: Command, _session_id: u64)
        -> Result<CommandResponse, RoutingError>
    {
        // 1. GameWorld 읽기 잠금 획득
        let world = self.game_world.read().await;

        // 2. 방 정보 조회 (하드코딩 room_id=1)
        let room_info = world.look_room(1)
            .ok_or_else(|| RoutingError::HandlerError("Room not found".to_string()))?;

        // 3. 응답 DTO 생성
        let response = LookResponse {
            room_name: room_info.name,
            room_description: room_info.description,
            exits: room_info.exits,
            players: room_info.players.into_iter().map(|p| PlayerSummary {
                id: p.id, name: p.name, level: p.level,
            }).collect(),
            npcs: room_info.npcs.into_iter().map(|n| NpcSummary {
                id: n.id, name: n.name, hp: n.hp, max_hp: n.max_hp,
            }).collect(),
        };

        // 4. 직렬화 및 응답 생성
        let payload = rmp_serde::to_vec(&response)
            .map_err(|e| RoutingError::HandlerError(e.to_string()))?;

        Ok(CommandResponse {
            id: _command.id,
            command_type: "look".to_string(),
            success: true,
            payload,
            error: None,
        })
    }
}
```

#### MoveHandler

```rust
impl CommandHandler for MoveHandler {
    async fn handle(&self, command: Command, _session_id: u64)
        -> Result<CommandResponse, RoutingError>
    {
        // 1. 페이로드 파싱
        let move_cmd: MoveCommand = rmp_serde::from_slice(&command.payload)
            .map_err(|e| RoutingError::HandlerError(e.to_string()))?;

        // 2. 쓰기 잠금 획득 (위치 변경)
        let mut world = self.game_world.write().await;

        // 3. 이동 실행 (하드코딩 character_id=1)
        match world.move_character(1, move_cmd.direction) {
            Ok(result) => {
                let response = MoveResponse {
                    success: true,
                    room_name: Some(result.room_name),
                    room_description: Some(result.room_description),
                    error: None,
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "move".to_string(),
                    success: true,
                    payload,
                    error: None,
                })
            }
            Err(e) => {
                // 실패 시 에러 응답
                let response = MoveResponse {
                    success: false,
                    room_name: None,
                    room_description: None,
                    error: Some(e.to_string()),
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "move".to_string(),
                    success: false,
                    payload,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}
```

#### AttackHandler

```rust
impl CommandHandler for AttackHandler {
    async fn handle(&self, command: Command, _session_id: u64)
        -> Result<CommandResponse, RoutingError>
    {
        // 1. 타겟 이름 추출 (바이트 → 문자열)
        let target_name = String::from_utf8_lossy(&command.payload).to_string();

        // 2. 쓰기 잠금
        let mut world = self.game_world.write().await;

        // 3. 전투 시작 (하드코딩 character_id=1)
        match world.start_combat(1, &target_name) {
            Ok(combat_info) => {
                let response = AttackResponse {
                    success: true,
                    damage: Some(combat_info.damage),
                    target_hp: Some(combat_info.target_hp),
                    message: Some(combat_info.message),
                    error: None,
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "attack".to_string(),
                    success: true,
                    payload,
                    error: None,
                })
            }
            Err(e) => {
                let response = AttackResponse {
                    success: false,
                    damage: None,
                    target_hp: None,
                    message: None,
                    error: Some(e.to_string()),
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "attack".to_string(),
                    success: false,
                    payload,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}
```

### 2.7 단계 8: 도메인 로직 실행

**전투 데미지 계산 예시 (`domain/src/combat.rs:48`):**

```rust
pub fn calculate_damage(attacker: &Character, target: &Character) -> u32 {
    use rand::Rng;

    let base_damage = attacker.stats.strength as f64;
    let defense = target.stats.constitution as f64 * 0.5;
    let raw_damage = (base_damage - defense).max(1.0);

    let mut rng = rand::thread_rng();
    let variance = raw_damage * 0.2;
    let final_damage = raw_damage + rng.gen_range(-variance..variance);
    final_damage.max(1.0) as u32
}
```

**데미지 공식:**
```
최종 데미지 = max(1, strength - (constitution × 0.5) ± variance)
variance = raw_damage × 0.2 × random(-1.0 ~ 1.0)
```

### 2.8 단계 9-10: 응답 전송

```rust
// CommandResponse를 Message로 감싸서 전송
fn command_response(response: CommandResponse) -> Message {
    let payload = rmp_serde::to_vec(&response).unwrap();
    Message::new(MessageType::CommandResponse, payload)
}

// 클라이언트에서 응답 수신 후 파싱
match response.message_type {
    MessageType::CommandResponse => {
        let resp: CommandResponse = rmp_serde::from_slice(&response.payload)?;
        if resp.success {
            // 성공 응답에 따라 구체 타입 파싱
            if let Ok(look_resp) = rmp_serde::from_slice::<LookResponse>(&resp.payload) {
                // 방 정보 표시
            } else if let Ok(move_resp) = rmp_serde::from_slice::<MoveResponse>(&resp.payload) {
                // 이동 결과 표시
            } else if let Ok(attack_resp) = rmp_serde::from_slice::<AttackResponse>(&resp.payload) {
                // 전투 결과 표시
            }
        } else {
            println!("Error: {}", resp.error.unwrap_or("Unknown".to_string()));
        }
    }
    MessageType::Error => {
        let error: ErrorResponse = rmp_serde::from_slice(&response.payload)?;
        println!("Error: {}", error.message);
    }
    _ => {}
}
```

---

## 3. 비동기 처리 전략

### 3.1 현재 비동기 구조

```
tokio::spawn (연결 태스크)
    │
    ├── reader.read_exact()   ← tokio I/O (non-blocking)
    │       │
    │       └── session.send()  ← mpsc::Sender (non-blocking)
    │
    └── incoming_rx.recv()    ← mpsc::Receiver (non-blocking)
            │
            └── writer.write_all()  ← tokio I/O (non-blocking)
```

**현재 블로킹 병목:**

```rust
// GameWorld에 대한 RwLock — 비동기 블로킹 가능
let world = self.game_world.read().await;   // 읽기 잠금
let mut world = self.game_world.write().await; // 쓰기 잠금
```

### 3.2 비동기 처리 개선 방안

**방법 1: Actor 모델 ( önerilen )**

```rust
// GameWorld를 Actor로 변환
pub struct GameWorldActor {
    state: GameWorld,
    command_rx: mpsc::Receiver<WorldCommand>,
    event_tx: broadcast::Sender<DomainEvent>,
}

pub enum WorldCommand {
    LookRoom { room_id: u32, reply: oneshot::Sender<Option<RoomInfo>> },
    MoveCharacter { character_id: u64, direction: Direction, reply: oneshot::Sender<Result<MoveResult, ApplicationError>> },
    StartCombat { attacker_id: u64, target_name: String, reply: oneshot::Sender<Result<CombatInfo, ApplicationError>> },
    // ...
}

impl GameWorldActor {
    pub async fn run(mut self) {
        while let Some(command) = self.command_rx.recv().await {
            match command {
                WorldCommand::LookRoom { room_id, reply } => {
                    let result = self.state.look_room(room_id);
                    let _ = reply.send(result);
                }
                WorldCommand::MoveCharacter { character_id, direction, reply } => {
                    let result = self.state.move_character(character_id, direction);
                    let _ = reply.send(result);
                }
                // ...
            }
        }
    }
}
```

**방법 2: RwLock → DashMap 전환**

```rust
// 현재: 단일 RwLock
pub struct GameWorld {
    characters: HashMap<u64, Character>,
    // ...
}

// 개선: DashMap으로 세분화된 잠금
pub struct GameWorld {
    characters: DashMap<u64, Character>,
    world: World,  // 변경 거의 없으므로 RwLock 유지
    combats: DashMap<u64, Combat>,
}
```

### 3.3 비동기 파이프라인

```
Client TCP
    │
    ▼
NetworkTask (tokio::spawn)
    │ decode
    ▼
SessionChannel (mpsc)
    │
    ▼
CommandRouterTask (tokio::spawn)
    │ route
    ▼
HandlerTask (tokio::spawn)
    │ process
    ▼
ResponseChannel (oneshot)
    │
    ▼
NetworkTask (tokio::spawn)
    │ encode
    ▼
Client TCP
```

---

## 4. 에러 복구

### 4.1 에러 분류 및 복구 전략

| 에러 레벨 | 예시 | 복구 전략 |
|-----------|------|-----------|
| **Level 1: 프레임 에러** | 불완전 프레임, CRC 불일치 | 프레임 버퍼 리셋, 재전송 요청 |
| **Level 2: 세션 에러** | 디코딩 실패, 알 수 없는 커맨드 | 에러 응답 전송, 세션 유지 |
| **Level 3: 비즈니스 에러** | 캐릭터 미존재, 이동 불가 | 에러 응답 전송, 상태 유지 |
| **Level 4: 시스템 에러** | DB 연결 실패, 메모리 부족 | 세션 종료, 재연결 유도 |
| **Level 5: 치명적 에러** | 프로세스 크래시 | 프로세스 재시작, 상태 복구 |

### 4.2 세션 레벨 에러 복구

```rust
// 연결 루프에서 에러 처리
loop {
    tokio::select! {
        result = reader.read_exact(&mut len_buf) => {
            match result {
                Ok(()) => {
                    // 정상 처리
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // 클라이언트 정상 종료
                    tracing::info!("Session {} client disconnected gracefully", session_id);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    // 연결 리셋 (클라이언트 비정상 종료)
                    tracing::warn!("Session {} connection reset", session_id);
                    break;
                }
                Err(e) => {
                    // 기타 IO 에러
                    tracing::error!("Session {} IO error: {}", session_id, e);
                    break;
                }
            }
        }
        // ...
    }
}

// 연결 종료 후 정리
session_manager.remove(session_id);
tracing::info!("Session {} cleaned up", session_id);
```

### 4.3 프레임 에러 복구

```rust
// 불완전 프레임 처리
pub fn decode(buf: &mut BytesMut) -> Result<Option<Message>, CodecError> {
    if buf.len() < 4 {
        return Ok(None);  // 프레임 헤더 불완전 — 더 데이터 대기
    }

    let total_len = {
        let mut peek = buf.clone();
        peek.get_u32() as usize
    };

    if total_len < 14 + 4 {  // 최소 프레임 크기
        return Err(CodecError::InvalidFrame("Frame too small".to_string()));
    }

    if total_len > 1024 * 1024 {  // 최대 프레임 크기 (1MB)
        return Err(CodecError::InvalidFrame("Frame too large".to_string()));
    }

    if buf.len() < total_len {
        return Ok(None);  // 프레임 불완전 — 더 데이터 대기
    }

    // 정상 디코딩
    // ...
}
```

### 4.4 커맨드 에러 복구

```rust
// CommandRouter에서 에러 처리
pub async fn route(
    &self,
    command: Command,
    session_id: u64,
) -> Result<CommandResponse, RoutingError> {
    let handler = self.handlers
        .get(&command.command_type)
        .ok_or_else(|| RoutingError::UnknownCommand(command.command_type.clone()))?;

    match handler.handle(command.clone(), session_id).await {
        Ok(response) => Ok(response),
        Err(RoutingError::HandlerError(msg)) => {
            // 핸들러 에러 — 에러 응답 반환
            tracing::error!(command = %command.command_type, error = %msg, "Handler error");
            Ok(CommandResponse {
                id: command.id,
                command_type: command.command_type,
                success: false,
                payload: vec![],
                error: Some(msg),
            })
        }
        Err(RoutingError::UnknownCommand(cmd)) => {
            // 알 수 없는 커맨드
            Ok(CommandResponse {
                id: command.id,
                command_type: cmd,
                success: false,
                payload: vec![],
                error: Some(format!("Unknown command: {}", cmd)),
            })
        }
    }
}
```

---

## 5. 멀티플레이어 동기화

### 5.1 현재 동기화 구조

현재는 모든 상태가 단일 `Arc<RwLock<GameWorld>>`에서 관리되므로 사실상 동기화가 자동으로 처리된다. 그러나 이 구조는 확장성에 한계가 있다.

```
┌──────────────────────────────────────┐
│          단일 GameWorld 인스턴스       │
│                                      │
│  Player A ──┐                        │
│  Player B ──┤──→ Arc<RwLock<>> ──→  World
│  Player C ──┘                        │
│                                      │
│  모든 변경사항이 즉시 다른 플레이어에게  │
│  보임 (read() 호출 시)               │
└──────────────────────────────────────┘
```

### 5.2 동기화 전략 (향후 확장)

**전략 1: 이벤트 기반 동기화**

```
Player A 이동
    │
    ▼
GameWorld.move_character()
    │
    ├── 상태 변경 (room_id 갱신)
    │
    └── DomainEvent 발생
         │
         ▼
    EventBus.publish(PlayerEnteredRoom)
         │
         ├── Player B에게 알림 (같은 방)
         ├── Player C에게 알림 (새 방)
         └── 로깅 핸들러
```

**전략 2: 방별 브로드캐스트**

```rust
pub struct RoomBroadcaster {
    room_sessions: DashMap<u32, Vec<u64>>,  // room_id → session_ids
}

impl RoomBroadcaster {
    /// 같은 방의 모든 플레이어에게 메시지 전송
    pub async fn broadcast_to_room(
        &self,
        room_id: u32,
        message: Message,
        exclude_session: Option<u64>,
    ) {
        if let Some(sessions) = self.room_sessions.get(&room_id) {
            for &session_id in sessions.iter() {
                if Some(session_id) != exclude_session {
                    if let Some(session) = self.session_manager.get(session_id) {
                        let _ = session.send(message.clone());
                    }
                }
            }
        }
    }

    /// 플레이어가 방에 입장했을 때
    pub fn player_entered(&self, room_id: u32, session_id: u64) {
        self.room_sessions
            .entry(room_id)
            .or_insert_with(Vec::new)
            .push(session_id);
    }

    /// 플레이어가 방을 떠났을 때
    pub fn player_left(&self, room_id: u32, session_id: u64) {
        if let Some(mut sessions) = self.room_sessions.get_mut(&room_id) {
            sessions.retain(|&s| s != session_id);
        }
    }
}
```

**전략 3: State Synchronization (대규모 확장)**

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Zone A     │     │  Zone B     │     │  Zone C     │
│  (Instances)│     │  (Instances)│     │  (Instances)│
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │                   │                   │
       └───────────────────┼───────────────────┘
                           │
                    ┌──────▼──────┐
                    │  State      │
                    │  Server     │
                    │  (Redis/DB) │
                    └─────────────┘
```

### 5.3 현재 동기화 제한사항

| 제한 | 설명 | 해결 방안 |
|------|------|-----------|
| 단일 인스턴스 | 서버 1개만 실행 가능 | 세션 스티커, Redis 공유 |
| 하드코딩된 character_id | 모든 플레이어가 ID=1로 동작 | 세션-캐릭터 매핑 |
| 방별 플레이어 수 제한 없음 | 메모리 소진 가능 | 방 인원 제한 |
| 실시간 전투 미동기화 | 턴 기반 미구현 | WebSocket/UDP 알림 |

---

## 6. 커맨드-이벤트 매핑 테이블

### 6.1 현재 구현된 커맨드

| 커맨드 | 입력 | DomainEvent 발생 | 응답 타입 |
|--------|------|------------------|-----------|
| `look` | 파라미터 없음 | — | `LookResponse` |
| `move` | `MoveCommand { direction }` | — | `MoveResponse` |
| `attack` | 타겟 이름 (바이트) | — | `AttackResponse` |
| `inventory` | 파라미터 없음 | — | `InventoryResponse` |
| `create_character` | `CreateCharacterCommand { name, class }` | — | `CreateCharacterResponse` |

### 6.2 향후 커맨드 확장

| 커맨드 | DomainEvent | 상태 |
|--------|-------------|------|
| `login` | `CharacterCreated` | ❌ |
| `logout` | — | ❌ |
| `say` | — | ❌ |
| `shout` | — | ❌ |
| `whisper` | — | ❌ |
| `buy` | `ItemAcquired` | ❌ |
| `sell` | `ItemRemoved` | ❌ |
| `use` | — | ❌ |
| `equip` | — | ❌ |
| `unequip` | — | ❌ |
| `join_guild` | — | ❌ |
| `auction_list` | — | ❌ |
| `auction_bid` | — | ❌ |

---

## 7. 레퍼런스

### 7.1 현재 관련 소스 파일

| 경로 | 라인 | 설명 |
|------|------|------|
| `core/network/src/lib.rs` | 53-76 | `accept_connections()` — 연결 수락 |
| `core/network/src/lib.rs` | 79-167 | `handle_connection()` — 연결 처리 |
| `core/protocol/src/codec.rs` | 93-127 | `decode_simple()` — 프레임 디코딩 |
| `core/protocol/src/message.rs` | 1-291 | 전체 프로토콜 메시지 정의 |
| `core/routing/src/lib.rs` | 39-49 | `CommandRouter::route()` — 커맨드 라우팅 |
| `core/session/src/session.rs` | 48-57 | `Session::send()`, `recv()` |
| `core/runtime/src/main.rs` | 55-91 | `run_server()` — 서버 초기화 |
| `core/runtime/src/main.rs` | 137-329 | `run_client()` — 클라이언트 루프 |
| `core/runtime/src/main.rs` | 342-569 | 모든 CommandHandler 구현 |
| `domain/src/combat.rs` | 48-59 | `calculate_damage()` — 데미지 계산 |
