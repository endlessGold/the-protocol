# 알려진 이슈 및 버그 목록

> The Protocol 프로젝트의 알려진 문제점 및 기술 부채 종합 목록
> 작성일: 2026-08-28

---

## Critical (즉시 수정 필요)

### 1. 코덱 decode() 버그

- **위치**: `core/protocol/src/codec.rs:51-93` (동일 파일 `core/codec.rs:51-91`)
- **현상**: `decode()` 메서드의 버퍼 관리가 불완전. `buf.split_to(payload_len)` 후 `buf.advance(4)`로 체크섬을 건너뛰지만, 실제 체크섬 값은 읽지 않고 무시됨. `decode_simple()`은 정상 동작
- **영향**: `decode()` 사용 시 체크섬 검증 불가, 버퍼 오프셋 어긋남 가능
- **수정 방안**: `decode_simple()`과 동일한 직선적 버퍼 읽기 패턴으로 재설계, 체크섬 검증 로직 추가
- **우선순위**: Critical

### 2. 네트워크 → 라우팅 연결 단절

- **위치**: `core/network/src/lib.rs:79-167`의 `handle_connection`
- **현상**: `NetworkManager`가 TCP 메시지를 읽고 세션의 `send()`로 전달하지만, `CommandRouter.route()`를 호출하지 않음. `run_server()`에서 `command_router`를 생성하지만 `NetworkManager`에 전달되지 않음
- **영향**: 클라이언트가 보낸 커맨드가 실제로 처리되지 않음. 세션에 메시지가 도착해도 핸들러가 실행되지 않아 모든 커맨드가 무시됨
- **수정 방안**: `NetworkManager::new()` 또는 `accept_connections()`에 `Arc<CommandRouter>` 주입, `handle_connection` 루프 내에서 MessageType::Command 수신 시 `command_router.route()` 호출 후 응답을 세션으로 전송
- **우선순위**: Critical

### 3. 캐릭터 ID 하드코딩

- **위치**: `core/runtime/src/main.rs:350-570`의 모든 CommandHandler (LookHandler, MoveHandler, AttackHandler, InventoryHandler)
- **현상**: 모든 핸들러에서 `character_id = 1`로 하드코딩. `session_id` 파라미터는 받지만 사용하지 않음
- **영향**: 멀티플레이어 불가. 모든 플레이어가 동일 캐릭터를 조작하며, 한 명의 이동/공격이 다른 모든 플레이어에게 영향
- **수정 방안**: `Session.player_id`에 실제 플레이어 ID 할당, 핸들러에서 `session_id`로 세션 조회 → `player_id` 추출 → 동적 캐릭터 ID 사용
- **우선순위**: Critical

### 4. core/ 루트 loose 파일과 crate 간 코드 충돌

- **위치**: `core/codec.rs`, `core/message.rs`, `core/lib.rs`, `core/session.rs`, `core/main.rs`, `core/tcp.rs`, `core/udp.rs`
- **현상**: core/ 루트에 crate 내부 파일과 동일하거나 유사한 7개의 .rs 파일 존재. `core/Cargo.toml`은 `protocol-session` 패키지를 정의하고 있어 `core/lib.rs`가 세션 모듈로 해석됨
- **영향**: 모호한 모듈 해석 가능성, 의존성 그래프 오해, 빌드 혼란
- **수정 방안**: 모든 loose .rs 파일 삭제 (crate 내부에 이미 올바르게 존재)
- **우선순위**: Critical

---

## High (빠른 수정 필요)

### 5. Session.player_id 미설정

- **위치**: `core/session/src/session.rs:14`, `core/network/src/lib.rs:116-118`
- **현상**: `Session` 구조체의 `player_id` 필드가 `Option<u64>`으로 정의되어 있으나, 핸드셰이크 완료 후 `set_player()`가 호출되지 않음
- **영향**: 세션에서 플레이어를 식별할 수 없음, 문제 3번(캐릭터 ID 하드코딩)의 근본 원인
- **수정 방안**: 로그인/인증 흐름 구현 후 `session.set_player(player_id)` 호출
- **우선순위**: High

### 6. 클라이언트 session_id = 0 하드코딩

- **위치**: `core/runtime/src/main.rs:178-238`, `clients/mud/src/main.rs:82-115`
- **현상**: 클라이언트가 커맨드 전송 시 `session_id: 0`으로 하드코딩. 서버에서 받은 `session_id`를 사용하지 않음
- **영향**: 서버에서 세션 기반 커맨드 추적 불가
- **수정 방안**: 핸드셰이크에서 받은 `hello_ack.session_id`를 저장하고 모든 커맨드에 포함
- **우선순위**: High

### 7. Command.session_id = 0 하드코딩

- **위치**: `core/runtime/src/main.rs:178` (`LookHandler`의 `session_id: 0`)
- **현상**: 클라이언트에서 전송하는 모든 Command의 session_id가 0
- **영향**: 서버가 명령의 출처 세션을 식별할 수 없음
- **수정 방안**: 서버에서 세션 ID를 Command에 주입하거나, 클라이언트가 자신의 세션 ID 포함
- **우선순위**: High

### 8. TCP/UDP 모듈 빈 파일

- **위치**: `core/network/src/tcp.rs`, `core/network/src/udp.rs`
- **현상**: 파일이 비어있고 주석 1줄만 포함. 모든 네트워크 로직이 `lib.rs`에 집중
- **영향**: TCP/UDP 전송 계층 분리 미비, UDP 지원 불가
- **수정 방안**: TCP 전용 로직을 `tcp.rs`로 분리, UDP 수신/송신 구현
- **우선순위**: High

---

## Medium (우선 수정 권장)

### 9. 코덱 encode()의 이중 직렬화

- **위치**: `core/protocol/src/codec.rs:32-48`
- **현상**: `encode()`에서 `message.payload`를 `rmp_serde::to_vec()`으로 이중 직렬화. Message 구조체 자체가 이미 serde를 지원하므로 payload의 Vec<u8>을 다시 직렬화하면 불필요한 래퍼가 추가됨
- **영향**: 페이로드 크기 증가, 디코딩 시 역직렬화 필요
- **수정 방안**: Message를 직접 `rmp_serde::to_writer()`로 직렬화하거나, payload가 이미 직렬화된 바이트인지 명확히 구분
- **우선순위**: Medium

### 10. 보안 모듈 has_runtime_capability 더미 구현

- **위치**: `core/security/src/lib.rs:216-219`
- **현상**: `has_runtime_capability()` 메서드가 항상 `true`를 반환
- **영향**: 런타임 기능 기반 접근 제어가 작동하지 않음
- **수정 방안**: 실제 RuntimeCapabilities 플래그와 비교하는 로직 구현
- **우선순위**: Medium

### 11. 스케줄러 tick()에서 Future 실행 안 함

- **위치**: `core/scheduler/src/lib.rs:102-122`
- **현상**: `tick()` 메서드가 실행 시간이 된 태스크를 감지하고 타이머를 업데이트하지만, 실제 `task`(Future)를 `tokio::spawn`하지 않음
- **영향**: 등록된 태스크가 실제로 실행되지 않음
- **수정 방안**: `tokio::task::spawn(task.task)` 호출 추가
- **우선순위**: Medium

### 12. 전투 시스템 1턴만 처리

- **위치**: `application/src/service.rs:161-228`
- **현상**: `start_combat()`이 한 번의 공격만 처리하고 끝남. `Combat` 구조체를 생성하지만 실제 전투 흐름을 관리하지 않음
- **영향**: 반복 공격 불가, 전투 상태 저장 불가
- **수정 방안**: 활성 전투 관리(HashMap<u64, Combat>), 턴 기반 공격/방어 루프 구현
- **우선순위**: Medium

### 13. NPC 데미지 처리의临时 Character 생성

- **위치**: `application/src/service.rs:189-210`
- **현상**: NPC의 데미지 계산을 위해 임시 `Character` 구조체를 생성하여 전달
- **영향**: NPC 고유 스탯 미적용, 확장 불가
- **수정 방안**: NPC도 Stats를 가지도록 도메인 모델 확장
- **우선순위**: Medium

### 14. 세션 하트비트 미구현

- **위치**: `core/session/src/`, `core/network/src/lib.rs`
- **현상**: 핸드셰이크에서 `heartbeat_interval_ms: 30000`을 전달하지만, 실제로 하트비트를 확인/전송하는 로직 없음
- **영향**: 비활성 세션 정리 불가, 유령 연결 누적
- **수정 방안**: 주기적 Ping/Pong 교환, 타임아웃 시 세션 제거
- **우선순위**: Medium

### 15. 플러그인 디렉토리 빈 구조

- **위치**: `plugins/auction/`, `plugins/character/`, `plugins/combat/`, `plugins/inventory/`
- **현상**: 각 디렉토리에 `src/`만 있고 `.rs` 파일이나 `Cargo.toml` 없음
- **영향**: 플러그인 시스템 테스트 불가
- **수정 방안**: 각 플러그인에 Cargo.toml + lib.rs + plugin.toml 매니페스트 추가
- **우선순위**: Medium

---

## Low (개선 사항)

### 16. Gateway 모드 미구현

- **위치**: `core/runtime/src/main.rs:331-340`
- **현상**: `run_gateway()`가 프린트만 하고 종료
- **영향**: 멀티 서버 아키텍처 불가
- **수정 방안**: 클라이언트 연결 수락 → 백엔드 서버 전달 구현
- **우선순위**: Low

### 17. 이벤트 시스템 미연결

- **위치**: `domain/src/event.rs`, `application/src/service.rs`
- **현상**: DomainEvent가 정의되고 도메인 로직에서 반환되지만, 이를 수신/처리하는 디스패처 없음
- **영향**: 이벤트 기반 확장 불가
- **수정 방안**: EventBus 또는 Observer 패턴 구현
- **우선순위**: Low

### 18. 서버/클라이언트 코드 중복

- **위치**: `core/runtime/src/main.rs` vs `clients/mud/src/main.rs`
- **현상**: 서버의 `run_client()`와 MUD 클라이언트의 `main()`이 거의 동일한 로직을 포함
- **영향**: 유지보수 비용 증가
- **수정 방안**: 공통 클라이언트 라이브러리(crate) 분리
- **우선순위**: Low

### 19. Direction 열거형 중복 정의

- **위치**: `core/protocol/src/message.rs:209-231`, `domain/src/world.rs:6-38`
- **현상**: Direction이 두 개의 crate에서 독립적으로 정의됨
- **영향**: 변환 함수 필요 (`proto_dir_to_domain`)
- **수정 방안**: 하나의 crate에서 정의하고 다른 쪽에서 re-export
- **우선순위**: Low

### 20. 미사용 import 및 unused warnings

- **위치**: 다수 파일
- **현상**: `_n`, `_checksum` 등 unused variable, 불필요한 import 존재
- **영향**: 컴파일 경고, 코드 가독성 저하
- **수정 방안**: `cargo fix` 또는 수동 정리
- **우선순위**: Low

---

## 기술 부채

### 중복 파일 정리

| 파일 | 상태 | 조치 |
|------|------|------|
| `core/codec.rs` | crate 내 `protocol/src/codec.rs`와 중복 | 삭제 |
| `core/message.rs` | crate 내 `protocol/src/message.rs`와 중복 | 삭제 |
| `core/lib.rs` | crate 내 `session/src/lib.rs`와 유사 | 삭제 |
| `core/session.rs` | crate 내 `session/src/session.rs`와 유사 | 삭제 |
| `core/main.rs` | `runtime/src/main.rs`와 유사 (약간의 차이) | 삭제 |
| `core/tcp.rs` | 빈 파일 | 삭제 |
| `core/udp.rs` | 빈 파일 | 삭제 |
| `the-protocol/` | 전체 프로젝트 복제 | 삭제 또는 .gitignore |

### 코드 품질

- **미사용 import 정리**: `use std::io::Write` 등 스코프 밖에서 사용되는 import
- **Ok(()) vs Ok(_n) 패턴 미통일**: 클라이언트 코드의 `Ok(_n)` 패턴 통일 필요
- **불필요한 clone**: runtime의 핸들러 등록 시 `game_world.clone()` 반복 → Arc로 충분
- **에러 처리 일관성**: `unwrap()` 사용 지점 제거, `?` 연산자 또는 `map_err` 사용
- **doc comments 부재**: 공개 API에 문서화 없음
