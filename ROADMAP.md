# The Protocol — 로드맵 & 체크리스트

> 최종 갱신: 2026-08-28
> 이 문서는 프로젝트 전체(Rust 게임 런타임 + entity-naming 서브프로젝트)의
> 우선순위와 진행 상태를 한곳에서 추적하기 위한 문서입니다. 세션이 바뀌어도
> 이 파일을 먼저 확인하면 "다음에 뭘 해야 하는지"를 알 수 있습니다.
>
> 세부 근거 문서:
> - Rust 런타임: [`docs/00-status/implementation-status.md`](docs/00-status/implementation-status.md),
>   [`docs/00-status/known-issues.md`](docs/00-status/known-issues.md)
> - entity-naming: [`entity-naming/docs/status/2026-08-28-session.md`](entity-naming/docs/status/2026-08-28-session.md)

체크박스 규칙: `[ ]` 미착수 · `[~]` 진행 중 · `[x]` 완료. 완료 항목은 지우지 말고
남겨서 이력을 유지합니다.

---

## A. Rust 코어 런타임 (`core/`, `domain/`, `application/`, `clients/`, ...)

전체 완성도 약 35% (2026-08-28 기준 감사 보고서). 멀티플레이어 동작에 필요한
연결 고리가 끊어져 있는 상태 — Phase 1을 끝내야 "여러 명이 접속해서 실제로
플레이할 수 있는" 최소 상태가 됩니다.

### Phase 1 — Critical: 멀티플레이어 가능하게 만들기
- [ ] **네트워크 → 라우팅 연결**: `core/network`가 `CommandRouter.route()`를
      호출하지 않아 클라이언트 커맨드가 실제로 처리되지 않음.
      (`core/network/src/lib.rs` `handle_connection`, `run_server()`)
- [ ] **캐릭터 ID 동적 할당**: 모든 핸들러가 `character_id = 1` 하드코딩 →
      전 플레이어가 같은 캐릭터를 조작하게 됨. `Session.player_id` 설정 로직과
      묶어서 해결 필요 (아래 항목과 연동).
- [ ] **`Session.player_id` 설정**: 핸드셰이크 완료 후 `set_player()`가 호출되지
      않음 — 캐릭터 ID 하드코딩 문제의 근본 원인.
- [ ] **코덱 `decode()` 버그 수정**: `core/protocol/src/codec.rs` — 체크섬을
      읽지 않고 건너뛰기만 함. `decode_simple()` 패턴으로 재설계 + 체크섬 검증.
- [ ] **`core/` 루트 loose 파일 정리**: `core/{codec,message,lib,session,main,tcp,udp}.rs`
      7개 파일이 crate 내부 파일과 중복/유사 — 전부 삭제 (진짜 소스는 각 crate
      `src/` 안에 있음).

### Phase 2 — High: 세션/클라이언트 정합성
- [ ] 클라이언트 `session_id = 0` 하드코딩 제거 (`hello_ack.session_id` 사용)
- [ ] `Command.session_id = 0` 하드코딩 제거 (서버가 세션 출처 식별 가능하게)
- [ ] `core/network/src/tcp.rs`, `udp.rs` 빈 파일 — TCP 로직 분리, UDP 구현

### Phase 3 — Medium: 품질/기능 완성도
- [ ] 코덱 `encode()` 이중 직렬화 제거
- [ ] `has_runtime_capability()` 더미(`true`) 구현을 실제 플래그 비교로 교체
- [ ] 스케줄러 `tick()`에서 `tokio::spawn(task.task)` 실제 실행
- [ ] 전투 시스템: 1턴만 처리하는 구조를 턴 기반 반복 전투로 확장
      (`HashMap<u64, Combat>` 활성 전투 관리)
- [ ] NPC 데미지 계산용 임시 Character 생성 제거 (NPC 자체 Stats 도입)
- [ ] 세션 하트비트/타임아웃 구현 (Ping/Pong, 유령 연결 정리)
- [ ] `plugins/{auction,character,combat,inventory}/`에 Cargo.toml + lib.rs +
      plugin.toml 뼈대 추가

### Phase 4 — Low: 확장
- [ ] Gateway 모드 구현 (현재 프린트만 하고 종료)
- [ ] 이벤트 디스패처/EventBus (DomainEvent를 실제로 소비하는 곳이 없음)
- [ ] 서버 `run_client()` / `clients/mud` 중복 로직 → 공통 클라이언트 crate로 분리
- [ ] `Direction` 중복 정의(`protocol` vs `domain`) 통합, 한쪽은 re-export
- [ ] `api/` (HTTP/WebSocket), `sdk/{csharp,typescript}` — 현재 빈 디렉토리
- [ ] `tests/` — 단위/통합 테스트 전무, 작성 필요
- [ ] 미사용 import / warning 정리, `unwrap()` → `?`/`map_err` 전환

---

## B. entity-naming (TypeScript pnpm 워크스페이스)

Rust 코어와 독립적인 서브프로젝트. 2026-08-28 세션에서 빌드/테스트가 깨져 있던
상태를 전부 정상화함 (`pnpm -r run build`, `typecheck`, `pnpm test` 31/31 통과,
CLI·Godot 어댑터 스모크 테스트 완료).

### 완료됨 (2026-08-28)
- [x] `packages/inference`, `packages/database` 스캐폴딩(package.json/tsconfig) 보완
- [x] `NamePool` SQL 오타 수정 (생성자 즉시 크래시하던 버그)
- [x] `core/engine.ts` 깨진 import 경로 수정 (`@entity-naming/database`로 정리)
- [x] `NamingEngine` 캐시 연동 (`setCache`/`generate()` 내 캐시 조회·저장)
- [x] `NamingEngine.setRouter()` 추가 (CLI `--ai` 플래그가 참조하던 미구현 메서드)
- [x] `PatternProvider` 어휘 병합 버그 수정 (person/npc/weapon 등 다수 타입 영향)
- [x] `providers/ai/router.ts` import 경로 수정 + `ProviderRouter` re-export 누락 수정
- [x] Gemini/Groq/Cerebras/OpenRouter `response.json()` 타입 오류 수정
- [x] `faker.ts` 로케일 처리(문자열→`LocaleDefinition`) 수정
- [x] Godot 어댑터 `/health`, `/providers` GET 허용 + 입력 검증 + `getProviders()` 공개 메서드
- [x] `LocalLLMProvider` 신규 작성 (llama.cpp/Ollama 어댑터를 네이밍 파이프라인에 연결)

### 남은 작업
- [ ] **`better-sqlite3` 네이티브 빌드**: 이 머신에 Python 없어서 `--ignore-scripts`로
      우회 중. `NamePool`(SQLite 네임 풀) 실동작 미검증. 옵션:
      Python 설치 / `better-sqlite3` 13.x로 업그레이드(prebuild 있음) /
      Node 내장 `node:sqlite`로 전환(네이티브 의존성 제거, 아키텍처 문서의
      "라즈베리파이/모바일에서도 동작" 목표에 더 부합).
- [ ] `LocalLLMProvider` 실제 GGUF 모델로 end-to-end 검증 (현재 타입만 검증됨)
- [ ] `packages/inference` 단위 테스트 작성
- [ ] AI 프로바이더(Gemini/Groq/Cerebras/OpenRouter) 실제 API 키로 동작 검증
- [ ] `cli`, `godot` 패키지 자체 테스트 추가 (현재 `tests/`는 core/providers만 커버)
- [ ] `NamePool.needsBackfill` → `preGenerate`/`backfill` 자동 트리거 연동
      (`autoBackfill` 옵션이 정의만 되어 있고 실제로 쓰이지 않음 — 확인 필요)

---

## 사용 방법

- 새 세션을 시작할 때: 이 파일 → 해당 섹션의 상세 근거 문서 순으로 확인.
- 항목을 끝내면 `[x]`로 바꾸고, 필요하면 "완료됨" 하위에 날짜와 함께 남깁니다.
- 새로운 이슈를 발견하면 해당 Phase/섹션에 체크박스로 추가하고, 근거가 되는
  파일 위치를 함께 적습니다 (다음 세션이 다시 찾아 헤매지 않도록).
