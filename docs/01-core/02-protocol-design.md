# 02. 프로토콜 설계 명세

## 1. 개요

The Protocol은 커스텀 바이너리 프로토콜을 사용하여 클라이언트-서버 간 통신한다.
이 문서는 메시지 포맷의 바이트 단위 명세, 모든 MessageType의 용도, 핸드셰이크 절차,
에러 핸들링, 그리고 개선이 필요한 사항을 상세히 기술한다.

## 2. 메시지 포맷 (바이트 단위 명세)

### 2.1 프레임 구조

```
┌────────────────────────────────────────────────────────────────────┐
│                        TCP Frame Layout                            │
├──────┬──────────┬──────────┬──────────┬────────────┬──────────────┤
│Length│ Version  │MessageID │  Type    │  Payload   │  Checksum    │
│4bytes│  1byte   │  8bytes  │  1byte   │  N bytes   │  4bytes      │
│u32BE │  u8      │  u64BE   │  u8      │  Vec<u8>   │  u32(CRC32)  │
├──────┼──────────┼──────────┼──────────┼────────────┼──────────────┤
│  0   │    4     │    5     │   13     │  14..N     │  N+1..N+4    │
└──────┴──────────┴──────────┴──────────┴────────────┴──────────────┘
                  ◄────── Total Length (4+N bytes) ──────►
```

### 2.2 필드 설명

| 오프셋 | 길이 | 필드 | 타입 | 설명 |
|--------|------|------|------|------|
| 0 | 4 | Length | u32 (Big Endian) | 전체 프레임 길이 (이 필드 포함). 값 = 14 + payload_len + 4 |
| 4 | 1 | Version | u8 | 프로토콜 버전 (현재: `PROTOCOL_VERSION = 1`) |
| 5 | 8 | MessageID | u64 (Big Endian) | 메시지 고유 ID. `rand::random()`으로 생성 |
| 13 | 1 | Type | u8 | MessageType 열거형의 정수 값 |
| 14 | N | Payload | Vec\<u8\> | rmp-serde로 직렬화된 데이터. N = Length - 18 |
| 14+N | 4 | Checksum | u32 (Big Endian) | Payload에 대한 CRC32 체크섬 |

### 2.3 인코딩 바이트 예시

Ping 메시지의 경우 (페이로드 비어 있음):

```
00 00 00 12    ← Length = 18 (14 + 0 + 4)
01             ← Version = 1
00 00 00 00 00 00 00 37  ← MessageID = 55 (임의)
20             ← Type = 0x20 (Ping)
               ← Payload = 없음
XX XX XX XX    ← Checksum = crc32fast::hash(&[])
```

### 2.4 코드 매핑 (`core/protocol/src/codec.rs`)

```rust
// 인코딩
pub fn encode(&self, message: &Message) -> Result<BytesMut, CodecError> {
    let payload = rmp_serde::to_vec(&message.payload)?;
    let checksum = crc32fast::hash(&payload);
    let total_len = 14 + payload.len() + 4;

    buf.put_u32(total_len as u32);     // 4 bytes
    buf.put_u8(message.version);       // 1 byte
    buf.put_u64(message.id);           // 8 bytes
    buf.put_u8(message.message_type as u8); // 1 byte
    buf.put_slice(&payload);           // N bytes
    buf.put_u32(checksum);             // 4 bytes
}
```

## 3. MessageType 열거형 전체 목록

### 3.1 정의 (`core/protocol/src/message.rs`)

| 값 | 이름 | 방향 | 용도 |
|----|------|------|------|
| `0x01` | **Command** | Client→Server | 클라이언트가 서버에 커맨드 요청 |
| `0x02` | **CommandResponse** | Server→Client | 커맨드 처리 결과 응답 |
| `0x10` | **Event** | Server→Client | 서버에서 발생한 이벤트 알림 (이동, 전투 등) |
| `0x11` | **EventAck** | Client→Server | 이벤트 수신 확인 (미사용) |
| `0x20` | **Ping** | 양방향 | 연결 유지 하트비트 요청 |
| `0x21` | **Pong** | 양방향 | Ping에 대한 응답 |
| `0x22` | **Hello** | Client→Server | 연결 초기화 및 인증 정보 전송 |
| `0x23` | **HelloAck** | Server→Client | 핸드셰이크 완료 확인 |
| `0x24` | **Disconnect** | 양방향 | 정상적인 연결 종료 |
| `0x25` | **Error** | Server→Client | 에러 메시지 전달 |
| `0x30` | **PluginMessage** | 양방향 | 플러그인 간 통신 (미사용) |
| `0x31` | **PluginResponse** | 양방향 | 플러그인 응답 (미사용) |

### 3.2 카테고리 분류

```
연결 관리:  Hello (0x22) → HelloAck (0x23) → Disconnect (0x24)
게임 로직:  Command (0x01) → CommandResponse (0x02)
이벤트:    Event (0x10) → EventAck (0x11)
유지보수:  Ping (0x20) → Pong (0x21)
에러:      Error (0x25)
플러그인:  PluginMessage (0x30) → PluginResponse (0x31)
```

### 3.3 메시지 구조체 상세

#### Hello (0x22)
```rust
struct Hello {
    protocol_version: u8,     // 클라이언트가 지원하는 프로토콜 버전
    client_version: String,   // 클라이언트 앱 버전 (env!("CARGO_PKG_VERSION"))
    client_type: ClientType,  // Game | MUD | Admin | Tool | Gateway | Internal
    auth_token: Option<String>, // 인증 토큰 (선택)
}
```

#### HelloAck (0x23)
```rust
struct HelloAck {
    session_id: u64,               // 서버가 할당한 세션 ID
    protocol_version: u8,          // 서버의 프로토콜 버전
    server_time: u64,              // 서버 타임스탬프 (UTC 밀리초)
    capabilities: Vec<String>,     // 서버가 지원하는 기능 목록
    heartbeat_interval_ms: u64,    // 하트비트 간격 (현재: 30000ms)
}
```

#### Command (0x01)
```rust
struct Command {
    id: u64,                    // 커맨드 고유 ID
    command_type: String,       // 커맨드 타입 ("look", "move", "attack" 등)
    session_id: u64,            // 발신자 세션 ID
    timestamp: u64,             // 클라이언트 타임스탬프
    payload: Vec<u8>,           // 커맨드별 상세 데이터
}
```

#### CommandResponse (0x02)
```rust
struct CommandResponse {
    id: u64,                    // 요청 커맨드 ID와 매칭
    command_type: String,       // 커맨드 타입
    success: bool,              // 성공 여부
    payload: Vec<u8>,           // 응답 데이터
    error: Option<String>,      // 에러 메시지 (실패 시)
}
```

#### Event (0x10)
```rust
struct Event {
    id: u64,                    // 이벤트 고유 ID
    event_type: String,         // 이벤트 타입
    timestamp: u64,             // 발생 시각
    source: String,             // 이벤트 발생 소스
    payload: Vec<u8>,           // 이벤트 데이터
    targets: Option<Vec<u64>>,  // 수신 대상 세션 ID 목록 (None = 전체)
}
```

#### ErrorResponse (0x25)
```rust
struct ErrorResponse {
    message: String,            // 에러 설명
}
```

### 3.4 커맨드별 페이로드 구조

| command_type | Request Payload | Response Payload |
|--------------|-----------------|------------------|
| `look` | 없음 (빈 바이트) | `LookResponse` |
| `move` | `MoveCommand { direction }` | `MoveResponse` |
| `attack` | 타겟 이름 (UTF-8 문자열) | `AttackResponse` |
| `inventory` | 없음 | `InventoryResponse` |
| `create_character` | `CreateCharacterCommand` | `CreateCharacterResponse` |

```rust
// Direction 열거형
enum Direction { North, South, East, West, Up, Down }

struct MoveCommand { direction: Direction }
struct MoveResponse {
    success: bool,
    room_name: Option<String>,
    room_description: Option<String>,
    error: Option<String>,
}

struct AttackCommand { target_id: u64 }
struct AttackResponse {
    success: bool,
    damage: Option<u32>,
    target_hp: Option<u32>,
    message: Option<String>,
    error: Option<String>,
}

struct LookResponse {
    room_name: String,
    room_description: String,
    exits: Vec<String>,
    players: Vec<PlayerSummary>,
    npcs: Vec<NpcSummary>,
}

struct PlayerSummary { id: u64, name: String, level: u32 }
struct NpcSummary { id: u64, name: String, hp: u32, max_hp: u32 }

struct InventoryResponse {
    items: Vec<InventoryItem>,
    gold: u64,
}

struct InventoryItem {
    item_id: u32,
    name: String,
    quantity: u32,
    item_type: String,
}
```

## 4. Handshake 프로토콜

### 4.1 시퀀스 다이어그램

```
Client                              Server
  │                                    │
  │  [TCP 연결 수립]                    │
  │  ─────────────────────────────────▶│
  │                                    │
  │  [Session 생성]                    │
  │  session_id = next_id()            │
  │  state = Connected                 │
  │                                    │
  │                                    │
  │  ── Hello (0x22) ────────────────▶│
  │     {                              │
  │       protocol_version: 1,         │
  │       client_version: "0.1.0",     │
  │       client_type: MUD,            │
  │       auth_token: None             │
  │     }                              │
  │                                    │
  │                                    │  [Hello 디코딩]
  │                                    │  [세션 상태 확인]
  │                                    │
  │  ◀── HelloAck (0x23) ─────────────│
  │     {                              │
  │       session_id: 42,              │
  │       protocol_version: 1,         │
  │       server_time: 1693200000000,  │
  │       capabilities: ["game"],      │
  │       heartbeat_interval_ms: 30000 │
  │     }                              │
  │                                    │
  │  [세션 상태: Connected →            │
  │   Authenticating → Authenticated]  │
  │                                    │
  │  ◀══════此后 Command/Event 교환 ═══▶│
```

### 4.2 핸드셰이크 구현 코드 (`core/network/src/lib.rs:79-120`)

```rust
async fn handle_connection(...) -> Result<(), NetworkError> {
    // 1. 세션 생성
    let session_id = session_manager.create_session(addr, TransportType::Tcp)?;

    // 2. Hello 메시지 수신
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let total_len = u32::from_be_bytes(len_buf) as usize;
    let mut frame = vec![0u8; total_len - 4];
    reader.read_exact(&mut frame).await?;

    // 3. HelloAck 전송
    let hello_ack = Message::hello_ack(session_id, vec!["game".to_string()]);
    writer.write_all(&codec.encode(&hello_ack)?).await?;

    // 4. 세션 상태 변경
    if let Some(mut session) = session_manager.get(session_id) {
        session.set_state(SessionState::Authenticated);
    }
}
```

### 4.3 클라이언트 핸드셰이크 구현 (`core/main.rs:112-140`)

```rust
// 1. Hello 전송
let hello = Message::hello(ClientType::MUD, None);
writer.write_all(&codec.encode(&hello)?).await?;

// 2. HelloAck 수신
reader.read_exact(&mut len_buf).await?;
// ... 프레임 조립 ...
let ack = ProtocolCodec::decode_simple(&mut buf)?;

match ack.message_type {
    MessageType::HelloAck => {
        let hello_ack: HelloAck = rmp_serde::from_slice(&ack.payload)?;
        println!("Connected! Session: {}", hello_ack.session_id);
    }
    _ => return Err(anyhow::anyhow!("Handshake failed")),
}
```

## 5. Command/Response/Event 메시지 구조

### 5.1 Command 라우팅 흐름

```
[Client]
    │
    │ Message::command(Command { command_type: "look", ... })
    │
    ▼
[NetworkManager: handle_connection]
    │ codec.decode_simple() → Message
    │ session.send(message)
    │
    ▼
[Session.incoming_rx]
    │ session.recv().await
    │
    ▼
[CommandRouter: route(command, session_id)]
    │ command.command_type == "look"
    │ → LookHandler.handle()
    │
    ▼
[GameWorld: look_room(room_id)]
    │ Room + Players + NPCs 조회
    │
    ▼
[CommandResponse { success: true, payload: LookResponse }]
    │ rmp_serde::to_vec(&response)
    │
    ▼
[ProtocolCodec.encode → TCP 전송]
```

### 5.2 Event 브로드캐스트 흐름

```
[Domain: 공격 실행 완료]
    │ DomainEvent::AttackExecuted { ... }
    │
    ▼
[Event { event_type: "attack", targets: None, ... }]
    │
    ▼
[SessionManager::broadcast(message, exclude)]
    │ DashMap 순회 → 각 Session.send()
    │
    ▼
[각 세션의 mpsc 채널 → TCP write 루프]
```

### 5.3 메시지 우선순위

| 우선순위 | 메시지 타입 | 설명 |
|---------|-------------|------|
| 1 (최고) | Error | 즉시 전송 |
| 2 | HelloAck | 핸드셰이크 완료 |
| 3 | CommandResponse | 커맨드 응답 |
| 4 | Event | 게임 이벤트 |
| 5 | Ping/Pong | 연결 유지 |
| 6 (최저) | PluginMessage | 플러그인 통신 |

## 6. 에러 핸들링 프로토콜

### 6.1 에러 타입

| 레벨 | 소스 | 예시 |
|------|------|------|
| **프로토콜 에러** | Codec | InvalidMessageType, ChecksumMismatch, Incomplete |
| **비즈니스 에러** | Routing | UnknownCommand, HandlerError |
| **도메인 에러** | Application | CharacterNotFound, NoExit, TargetDead |
| **네트워크 에러** | Network | Io, Closed, Session |
| **세션 에러** | Session | NotFound, Closed |

### 6.2 에러 응답 구조

```rust
// 서버 → 클라이언트 에러 메시지
Message::error("Room not found".to_string())
// 내부 구조: ErrorResponse { message: String }
// MessageType: Error (0x25)

// 커맨드 실패 응답
CommandResponse {
    id: command.id,
    command_type: "move".to_string(),
    success: false,
    payload: vec![],
    error: Some("No exit in that direction".to_string()),
}
```

### 6.3 에러 핸들링 전략

```
[네트워크 에러] → 세션 제거 + 로그 기록
[프로토콜 에러] → 연결 종료 (복구 불가)
[비즈니스 에러] → Error 메시지 전송 (연결 유지)
[도메인 에러]   → CommandResponse.error에 포함
```

### 6.4 CodecError 처리

```rust
#[derive(Debug, Error)]
pub enum CodecError {
    Io(std::io::Error),              // TCP 읽기/쓰기 실패
    InvalidMessageType(u8),           // 알 수 없는 메시지 타입
    ChecksumMismatch,                // CRC32 불일치 (미구현)
    Deserialization(String),          // rmp-serde 디코딩 실패
    Incomplete,                      // 프레임 불완전 (데이터 부족)
}
```

## 7. 현재 구현 vs 설계 차이점

### 7.1 현재 구현된 것

| 기능 | 상태 | 위치 |
|------|------|------|
| TCP 수신/송신 | ✅ 구현 완료 | `core/network/src/lib.rs` |
| Length-prefix 프레이밍 | ✅ 구현 완료 | `core/protocol/src/codec.rs` |
| Hello/HelloAck 핸드셰이크 | ✅ 구현 완료 | `core/network/src/lib.rs` |
| Command/CommandResponse | ✅ 구현 완료 | `core/main.rs` (핸들러) |
| Ping/Pong | ⚠️ 메시지 정의만 | MessageType에 포함, 핸들러 미구현 |
| Disconnect | ⚠️ 메시지 정의만 | MessageType에 포함, 처리 미구현 |
| CRC32 체크섬 | ⚠️ 인코딩 시 생성 | 디코딩 시 검증 로직 미완성 |
| rmp-serde 직렬화 | ✅ 구현 완료 | 모든 메시지에 적용 |

### 7.2 설계만 되어 있는 것

| 기능 | 상태 | 필요 작업 |
|------|------|-----------|
| 압축 (Compression) | 미구현 | LZ4/zstd 선택 및 구현 |
| 암호화 (Encryption) | 미구현 | TLS 래핑 또는 Payload 암호화 |
| UDP 시퀀스 | 미구현 | 시퀀스 번호 및 누락 복구 |
| 세션 토큰 검증 | 미구현 | auth_token 검증 로직 |
| 메시지 크기 제한 | 미구현 | 최대 페이로드 크기 검증 |
| Rate Limiting | 미구현 | 세션별 요청 제한 |

### 7.3 구현 격차 분석

```
설계 (Design)                    구현 (Implementation)
─────────────────────────────    ─────────────────────────────
✓ 바이너리 프로토콜               ✅ 완료
✓ Length-prefix 프레이밍          ✅ 완료
✓ 12가지 MessageType             ✅ 완료
✓ Handshake 프로토콜              ✅ 완료
✓ Command/Response 패턴           ✅ 완료
✗ 메시지 압축                     미구현
✗ 전송 암호화                     미구현
✗ UDP 전송 레이어                  미구현
✗ 시퀀스 번호 관리                 미구현
✗ 하트비트 자동 처리               미구현
✗ 세션 타임아웃                    미구현
✗ 체크섬 검증                      미구현 (생성만)
```

## 8. 개선 필요 사항

### 8.1 압축 (Compression)

**목적**: 대용량 페이로드 (InventoryResponse, LookResponse)의 전송 효율 향성

**권장 방식**:
- 페이로드 > 256 bytes인 경우에만 압축 적용
- LZ4 (빠른 속도) 또는 Zstandard (높은 압축률) 선택
- 헤더에 Compression 플래그 추가

```
현재:  [Length][Version][ID][Type][Payload][Checksum]
개선:  [Length][Version][ID][Type][Flags][Payload][Checksum]
                                ↑
                           압축/암호화 플래그
```

### 8.2 암호화 (Encryption)

**목적**: 전송 구간 데이터 보안 (TLS 미사용 시)

**권장 방식**:
- **방법 A**: TCP 위에 TLS 래핑 (권장) - Rustls 또는 NativeTLS
- **방법 B**: 페이로드 레벨 암호화 (AES-256-GCM) - 특정 사용 사례에 적합
- Handshake 시 키 교환 (Diffie-Hellman 또는 Pre-shared Key)

### 8.3 UDP 전송

**목적**: 지연 시간 민감한 데이터 (위치 업데이트, 상태 동기화)

**설계**:
```
UDP Frame Layout:
┌──────────┬──────────┬──────────┬──────────┬────────────┐
│ Sequence │ Version  │MessageID │  Type    │  Payload   │
│ 4bytes   │  1byte   │  8bytes  │  1byte   │  N bytes   │
└──────────┴──────────┴──────────┴──────────┴────────────┘
(Length 불필요 - UDP는 메시지 단위 수신)

불량 복구:
- 시퀀스 번호 기반 누락 감지
- NACK (Negative Acknowledgement) 요청
- 선택적 재전송
```

### 8.4 하트비트/Keepalive

**목적**: 연결 생존 확인 및 타임아웃 감지

**현재**: HelloAck에 `heartbeat_interval_ms: 30000` 포함 (미사용)

**개선 설계**:
```
[Client] ──Ping──▶ [Server]
[Client] ◀──Pong── [Server]

규칙:
- heartbeat_interval_ms 간격으로 Ping 전송
- 3회 연속 Pong 미수신 시 연결 종료
- 서버 측: heartbeat_interval_ms * 2.5 타임아웃
```

### 8.5 시퀀스 번호

**목적**: 메시지 순서 보장 및 누락 감지 (UDP용)

**설계**:
- HelloAck에서 `initial_sequence: u32` 할당
- 이후 모든 메시지에 시퀀스 번호 포함
- 수신 측에서 비순서 메시지 감지 및 로그

## 9. 테스트 시나리오 목록

### 9.1 프로토콜 레벨 테스트

| # | 시나리오 | 기대 결과 |
|---|---------|-----------|
| 1 | Ping 메시지 인코딩/디코딩 라운드트립 | 원본과 동일 |
| 2 | 최소 크기 메시지 (빈 페이로드) | 정상 처리 |
| 3 | 최대 크기 메시지 (1MB 페이로드) | 정상 처리 |
| 4 | 잘못된 MessageType (0xFF) | `InvalidMessageType` 에러 |
| 5 | 불완전 프레임 (Length만 수신) | `Incomplete` 반환 |
| 6 | 버전 불일치 메시지 | 연결 거부 |
| 7 | 체크섬 불일치 | `ChecksumMismatch` 에러 |

### 9.2 핸드셰이크 테스트

| # | 시나리오 | 기대 결과 |
|---|---------|-----------|
| 1 | 정상 Hello → HelloAck | 세션 ID 할당 |
| 2 | Hello 없이 Command 전송 | 연결 종료 |
| 3 | 잘못된 버전으로 Hello | 에러 응답 |
| 4 | 동시 연결 1000개 | 모두 정상 핸드셰이크 |
| 5 | 핸드셰이크 중 연결 끊김 | 세션 정리 |
| 6 | auth_token 포함 Hello | 토큰 검증 (미구현) |

### 9.3 커맨드 라우팅 테스트

| # | 시나리오 | 기대 결과 |
|---|---------|-----------|
| 1 | 등록된 커맨드 ("look") | 핸들러 호출 성공 |
| 2 | 미등록 커맨드 ("fly") | `UnknownCommand` 에러 |
| 3 | 유효하지 않은 페이로드 | `HandlerError` |
| 4 | 세션 미존재 세션 ID | 세션 에러 |
| 5 | 동시 커맨드 처리 (10개) | 모두 정상 응답 |

### 9.4 네트워크 테스트

| # | 시나리오 | 기대 결과 |
|---|---------|-----------|
| 1 | 정상 연결/데이터 교환 | 양방향 통신 |
| 2 | 클라이언트 비정상 종료 (RST) | 세션 자동 정리 |
| 3 | 네트워크 파티션 후 복구 | 타임아웃 처리 |
| 4 | 대용량 동시 전송 (스�밍) | 에러 처리 |
| 5 | Nagle 알고리즘 비활성화 확인 | `set_nodelay(true)` |

### 9.5 직렬화 테스트

| # | 시나리오 | 기대 결과 |
|---|---------|-----------|
| 1 | LookResponse rmp-serde 라운드트립 | 정상 직렬화/역직렬화 |
| 2 | MoveCommand (모든 Direction) | 정상 처리 |
| 3 | Unicode 캐릭터 이름 ("한국어이름") | 정상 인코딩 |
| 4 | 빈 inventory | 빈 목록 반환 |
| 5 | 대형 inventory (100+ 아이템) | 정상 처리 |

### 9.6 통합 테스트

| # | 시나리오 | 기대 결과 |
|---|---------|-----------|
| 1 | Client → Server 전체 커맨드 플로우 | end-to-end 성공 |
| 2 | 멀티 클라이언트 이동 시 브로드캐스트 | 모든 클라이언트에 이벤트 수신 |
| 3 | 전투 커맨드 전체 플로우 | 데미지 계산 및 응답 |
| 4 | 캐릭터 생성 → 이동 → 공격 시퀀스 | 연속 동작 성공 |
| 5 | 긴 세션 (1시간 이상) | 연결 유지 |

## 10. 요약

The Protocol의 프로토콜은 **단순하고 효율적인 바이너리 포맷**을 기반으로 설계되었다.
현재 TCP 기반의 핸드셰이크, 커맨드/응답, 이벤트 브로드캐스트가 구현되어 있으며,
향후 압축, 암호화, UDP, 하트비트 등이 추가될 예정이다. CRC32 체크섬은 인코딩 시
생성되지만 디코딩 시 검증이 미완성되어 있어 이를 우선적으로 보완해야 한다.
