# 구현 현황

> 최종 갱신: 2026-08-29
>
> **이 문서는 스냅샷이다. 실제 동작의 유일한 출처는 코드다.**
> 이 파일의 이전 판은 초기 커밋 이후 한 번도 갱신되지 않아, 존재하지도 않는
> 타입(`DefaultPluginRuntime`, `PluginRuntime`)을 설명하고 이미 고쳐진 버그를
> 미해결로 표시하고 있었다. 뭔가 이상하면 `docs/`가 아니라 해당 crate의
> `src/`를 읽을 것.
>
> 할 일 목록은 여기가 아니라
> [GitHub Issues](https://github.com/endlessGold/the-protocol/issues)에 있다.

| 기호 | 의미 |
|---|---|
| ✅ | 동작하고 검증됨 |
| 🔧 | 동작하지만 미완 |
| 🐛 | 알려진 버그 있음 |
| ❌ | 비어있음 |

## 검증 수단

CI(`.github/workflows/ci.yml`)가 매 푸시마다:
`cargo build --workspace` · `cargo test --workspace` · `bindings/godot` 빌드
(Linux+Windows) · **실제 헤드리스 Godot에서 GDExtension 로드 + GDScript 구동 +
실서버 상대 네트워크 왕복 테스트**. `fmt-and-clippy`는 현재 advisory.

## 코어

| crate | 상태 | 비고 |
|---|---|---|
| `core/protocol` | ✅ | 코덱 encode/decode 정상, 체크섬 검증됨. 테스트 10개(와이어 포맷 계약 6 + 코덱 4) |
| `core/network` | ✅ | `CommandRouter` 연결됨, Command/Ping/Disconnect 처리. `tcp.rs`/`udp.rs`는 1줄 주석 스텁이고 UDP 전송은 없음 |
| `core/session` | 🔧 | 세션 CRUD·`player_id` 바인딩·room 조회 동작. **하트비트/타임아웃 없음** — `Session::touch()` 호출부 0개 |
| `core/routing` | 🔧 | 문자열 커맨드 → 핸들러 디스패치 동작. 플러그인 폴백 없음 |
| `core/presentation` | ✅ | `PresentationCommand`/`translate_event`. 테스트 7개. JSON 형태가 Godot 브릿지와 계약으로 고정됨 |
| `core/plugin` | 🔧 | wasmtime 엔진·14개 host function·매니페스트 검증은 실제 동작. **다만 `handle_command`/`handle_event` 호출부가 0개라 플러그인은 로드만 되고 아무것도 하지 않음.** 타이머 없음. `HostState`가 `GameWorld`와 완전히 분리된 별도 저장소 |
| `core/security` | 🔧 | `check_permission`은 진짜 로직이지만 crate 밖 호출부 0개. `has_runtime_capability()`는 무조건 `true` 반환하는 스텁. `CapabilityManager`는 생성만 되고 쓰이지 않음 |
| `core/scheduler` | ❌ | **죽은 코드.** `tick()`이 등록된 Future를 실행/spawn하지 않고, `Scheduler::new`는 워크스페이스 전체에서 호출부 0개 |
| `core/observability` | ✅ | `RUST_LOG` 기반 tracing 초기화 |
| `core/runtime` | 🔧 | 서버 모드 동작(핸들러 5개 + 이벤트 디스패치). Gateway 모드는 "not yet implemented" 출력 스텁. `run_client()`는 `clients/mud`와 중복이며 아래 버그 공유 |

## 도메인 / 애플리케이션

| 영역 | 상태 | 비고 |
|---|---|---|
| `domain` | ✅ | `Combatant` 트레이트로 `Character`/`Npc` 통합. NPC가 실제 `level/attack/defense` 보유. `World`에 NPC 추가/이동/제거(`Room.npc_ids`와 `Npc.room_id` 동기 유지) |
| `application` | 🔧 | `GameWorld`가 이벤트 큐(`pending_events`/`drain_events`)를 갖고 캐릭터·NPC·전투 변경 시 `DomainEvent` 발행. **전투는 1회성** — `combats` 맵이 쓰기 전용이고 NPC 반격·턴 지속·사망 처리가 없음 |

## 클라이언트 / 바인딩

| 영역 | 상태 | 비고 |
|---|---|---|
| `bindings/godot` | ✅ | gdext 0.5.5(api-4-4). 실제 Godot에서 로드·인스턴스화·구동됨을 CI가 검증 |
| `godot-client` (별도 저장소) | ✅ | 커맨드 인터프리터·TCP 클라이언트·플레이 가능한 씬. GDScript 테스트 3종이 CI에서 통과 |
| `clients/mud`, `runtime run_client` | 🐛 | **깨져 있음.** 엄격한 요청/응답 루프라, 서버가 푸시하는 비동기 `Event`를 커맨드 응답 자리에서 소비한다. 둘은 서로 거의 완전한 중복 |

## 비어있음

`api/` · `sdk/csharp/` · `sdk/typescript/` · 워크스페이스 루트 `tests/` ·
`plugins/{auction,character,combat,inventory}` — 전부 파일 0개.
실제 존재하는 플러그인은 `plugins/hello-world`(스텁 WAT) 하나뿐.

## 테스트 현황

러스트 유닛 테스트 21개 — `core/protocol`(10), `core/presentation`(7),
`core/plugin/manifest`(4). **`domain`·`application`은 0개**로, 최근 변경이 집중된
영역(전투·NPC·엔티티 ID·이벤트 발행)이 무테스트다.
통합 테스트는 러스트 쪽엔 없고 GDScript(`godot-client/tests/`) 3종이 CI에서
서버 상대로 돈다.
