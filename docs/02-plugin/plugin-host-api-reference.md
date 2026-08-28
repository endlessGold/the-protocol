# 플러그인 호스트 API 레퍼런스 (실제 코드 기준)

> 작성일: 2026-08-28
> 근거: `core/plugin/src/{engine,host,manifest,state,error}.rs`, `plugins/hello-world/*`
> 이 문서는 **실제로 동작하는 코드**만 기준으로 작성했습니다. `docs/02-plugin/`
> 아래 다른 설계 문서들과 다른 부분이 있다면, 9번 섹션에서 그 차이를 명시적으로
> 정리했습니다 — 설계 문서 쪽이 아직 구현되지 않은 계획입니다.

## 0. 한 줄 정정

> "플러그인이 SDK의 API를 호출한다" — 맞습니다. 다만 지금은 **언어별 SDK
> 패키지가 존재하지 않습니다.** `sdk/typescript/`, `sdk/csharp/`는 빈
> 디렉토리입니다. 플러그인이 실제로 호출하는 건 **14개의 저수준 WASM 함수
> (host function)** 이고, 지금 이 문서가 다루는 게 바로 그 계약입니다.

## 1. 큰 그림

플러그인은 WASM 모듈로 컴파일되어야 합니다. 런타임(`core/plugin`, Rust +
[wasmtime](https://wasmtime.dev/))이 그 모듈을 로드하면서 `plugin_host`라는
이름의 모듈 네임스페이스 아래 14개 함수를 링크해 줍니다. 플러그인은 이 함수를
**import**해서 호출하고, 런타임은 반대로 플러그인이 **export**한 9개 함수를
호출합니다. 이 두 방향의 계약이 전부입니다 — 그 이상의 마법은 없습니다.

```
플러그인이 호출 (Host Function, 14개)   런타임이 호출 (Plugin Export, 9개)
  log, storage_get/set/delete             memory, allocate_buffer, free_buffer
  emit_event                              plugin_init/enable/disable/unload
  player_get/update                       handle_command, handle_event
  inventory_get/add_item/remove_item
  combat_start/action
  send_to_client, broadcast_to_room
```

## 2. 플러그인 생명주기

`PluginManifest`의 `PluginState`는 다섯 단계입니다: `Discovered → Loaded →
Initialized → Enabled → Disabled`. `PluginEngine`의 메서드가 각 전이를
일으키고, 그때마다 플러그인의 대응 export를 호출합니다.

| 단계 전이 | 호출하는 `PluginEngine` 메서드 | 호출되는 플러그인 export |
|---|---|---|
| (파일 시스템) → Discovered | `discover()` | 없음 — `plugin.toml`만 읽음 |
| Discovered → (컴파일됨) | `compile()` | 없음 — `api_version` 검증 후 WASM 컴파일만 |
| → Loaded | `instantiate()` | 없음 — 인스턴스 생성, host function 링크 |
| Loaded → Initialized | `initialize()` | `plugin_init() -> i32` |
| Initialized → Enabled | `enable()` | `plugin_enable() -> i32` |
| Enabled → Disabled | `disable()` | `plugin_disable() -> i32` |
| (제거) | `unload()` | (Enabled였다면 `plugin_disable()` 먼저) `plugin_unload()` |

모든 lifecycle export는 `0`을 반환해야 성공으로 처리됩니다. 0이 아니면 그
단계로 전이가 실패합니다. 상태를 건너뛰면(예: Loaded 상태에서 `enable()`
호출) `PluginError::Lifecycle` 에러가 납니다 — `PluginEngine`이 순서를
강제합니다.

실제 서버 부팅 시퀀스는 `core/runtime/src/main.rs`의 `run_server()`에서
`discover → compile → instantiate → initialize → enable`을 플러그인마다
순서대로 실행합니다.

## 3. 메모리 마샬링 컨벤션 — 가장 중요한데 어디에도 안 적혀있던 부분

WASM 선형 메모리는 **플러그인이 소유**합니다. 호스트(런타임)는 그 메모리를
직접 읽고 쓸 수 있지만, 메모리를 할당/해제하는 함수(`allocate_buffer`,
`free_buffer`)는 플러그인이 export해야 합니다 — 즉 "이 메모리에 뭘 좀
써도 될까?"를 항상 플러그인에게 물어봐야 합니다.

**문자열/바이트 인자를 host function에 넘길 때** (예: `log`,
`storage_set`): 플러그인이 자기 메모리 어딘가에 데이터를 쓰고, 그 시작
주소(`ptr: u32`)와 길이(`len: u32`)를 함수 인자로 넘깁니다. 런타임은
`caller.get_export("memory")`로 플러그인 메모리에 접근해서 그 위치를 읽습니다.

**가변 길이 데이터를 host function이 "반환"할 때** (`storage_get`,
`player_get`, `inventory_get`): 반환 타입은 `i64`이지만 실제로는 **포인터와
길이를 한 정수에 패킹**한 값입니다.

```rust
// core/plugin/src/host.rs
Ok(((buf_ptr as i64) << 32) | (len as i64))
```

풀어보면: 상위 32비트 = 버퍼 포인터, 하위 32비트 = 바이트 길이. 이 버퍼는
런타임이 **플러그인의 `allocate_buffer`를 호출**해서 플러그인 메모리 안에
만든 것입니다 — 즉 소유권이 플러그인에게 넘어갑니다. **다 읽었으면 플러그인이
직접 `free_buffer`를 호출해서 해제해야 합니다.** 런타임은 대신 해제해주지
않습니다.

```
1. 플러그인 → 호스트: player_get(player_id)
2. 호스트: DashMap에서 PlayerData 조회, rmp_serde로 MessagePack 직렬화
3. 호스트 → 플러그인: allocate_buffer(len) 호출 → ptr 받음
4. 호스트: 플러그인 메모리의 ptr 위치에 직렬화된 바이트를 씀
5. 호스트 → 플러그인: (ptr << 32) | len 반환
6. 플러그인: 그 ptr/len으로 자기 메모리를 읽어서 MessagePack 역직렬화
7. 플러그인: 다 썼으면 free_buffer(ptr, len) 호출 (호스트는 안 해줌)
```

값이 없는 경우(예: 존재하지 않는 플레이어)는 함수마다 다른 sentinel
값을 반환합니다 — 5번 섹션 표를 참고하세요.

## 4. Host Function 레퍼런스 (플러그인이 호출)

모든 host function은 WASM import 모듈명 `"plugin_host"` 아래 등록되어
있습니다 (`core/plugin/src/engine.rs`의 `build_linker()`).

| 함수 | 시그니처 | 설명 | 실패/빈 값 시 반환 |
|---|---|---|---|
| `log` | `(level: i32, ptr: u32, len: u32) -> ()` | UTF-8 메시지를 로깅. `level`: 0=trace, 1=debug, 2=info, 3=warn, 4=error (그 외는 info로 처리) | 반환값 없음 (실패해도 조용히 무시) |
| `storage_get` | `(key_ptr, key_len: u32) -> i64` | 플러그인 전용 KV 스토리지 조회. 키는 내부적으로 `"{플러그인명}.{key}"`로 네임스페이스됨 — 다른 플러그인 데이터를 볼 수 없음 | 키 없음: `-1` |
| `storage_set` | `(key_ptr, key_len, val_ptr, val_len: u32) -> i32` | 값 저장 (덮어쓰기) | 항상 `0` |
| `storage_delete` | `(key_ptr, key_len: u32) -> i32` | 키 삭제 | 항상 `0` (키가 없어도 에러 아님) |
| `emit_event` | `(type_ptr, type_len, data_ptr, data_len: u32) -> i32` | 이벤트 발행. 현재 `"{type}:{data}"` 문자열로 합쳐서 내부 이벤트 큐에 쌓기만 함 — 구독/디스패치 메커니즘은 없음(8번 참고) | 항상 `0` |
| `player_get` | `(player_id: i64) -> i64` | 플레이어 데이터 조회 (packed ptr/len, 3번 섹션 참고). MessagePack(`PlayerData`) | 플레이어 없음: `-20` |
| `player_update` | `(player_id: i64, data_ptr, data_len: u32) -> i32` | 플레이어 데이터 갱신. MessagePack으로 넘긴 `PlayerData` 전체를 덮어씀(부분 갱신 아님) | 역직렬화 실패 시 `-1` |
| `inventory_get` | `(player_id: i64) -> i64` | 인벤토리 조회 (packed ptr/len). MessagePack(`Vec<InventoryEntry>`) | 없으면 빈 배열 (에러 아님) |
| `inventory_add_item` | `(player_id, item_id: i64, count: i32) -> i32` | 아이템 추가/수량 증가 | 항상 `0` |
| `inventory_remove_item` | `(player_id, item_id: i64, count: i32) -> i32` | 아이템 제거/수량 감소 | 수량 부족: `-1`, 아이템 없음: `-2` |
| `combat_start` | `(attacker_id, defender_id: i64) -> i64` | 전투 세션 생성, `combat_id` 반환. **`application/src/service.rs`의 게임 전투와는 별개 저장소** — 캐릭터 HP 등에 영향 없음 (9번 참고) | (현재 항상 성공) |
| `combat_action` | `(combat_id: i64, action_ptr, action_len: u32) -> i32` | 턴 카운터만 증가. **행동을 파싱하거나 데미지를 계산하지 않는 스텁** (9번 참고) | 전투 없음: `-30` |
| `send_to_client` | `(player_id: i64, msg_ptr, msg_len: u32) -> i32` | 특정 플레이어에게 메시지 큐잉 | 항상 `0` |
| `broadcast_to_room` | `(room_id: i64, msg_ptr, msg_len: u32) -> i32` | 같은 방(`room_id`)의 모든 플레이어에게 메시지 큐잉 | 항상 `0` |

**주의**: `docs/02-plugin/03-plugin-api-contract.md`는 `-1`~`-59`대의 정교한
에러 코드 체계를 설계해두었지만, 실제 코드는 위 표의 값들만 즉흥적으로
사용합니다. 체계적인 코드를 기대하지 마세요.

## 5. Plugin Export 계약 (런타임이 호출)

WASM 모듈이 반드시 export해야 하는 것들입니다. 이름이나 시그니처가 다르면
런타임이 `PluginError::FunctionNotFound`로 실패합니다.

| Export | 시그니처 | 호출 시점 |
|---|---|---|
| `memory` | (선형 메모리) | 항상 필요 — 이게 없으면 아무 host function도 동작 못함 |
| `allocate_buffer` | `(size: i32) -> i32` (포인터) | 런타임이 플러그인 메모리에 데이터를 써야 할 때마다 |
| `free_buffer` | `(ptr: i32, size: i32) -> ()` | 런타임이 커맨드/이벤트 인자용으로 할당한 버퍼를 정리할 때 (플러그인 자신도 3번 섹션 절차대로 호출해야 함) |
| `plugin_init` | `() -> i32` | 2번 섹션 |
| `plugin_enable` | `() -> i32` | 2번 섹션 |
| `plugin_disable` | `() -> i32` | 2번 섹션 |
| `plugin_unload` | `() -> i32` | 2번 섹션 |
| `handle_command` | `(cmd_ptr, cmd_len, args_ptr, args_len: i32, player_id: i64) -> i32` | `PluginEngine::handle_command()` 호출 시 |
| `handle_event` | `(type_ptr, type_len, data_ptr, data_len: i32) -> i32` | `PluginEngine::handle_event()` 호출 시 |

## 6. `plugin.toml` 매니페스트

```toml
name = "hello-world"
version = "0.1.0"
description = "Minimal test plugin for The Protocol plugin engine"
api_version = "0.1.0"          # RUNTIME_API_VERSION과 MAJOR가 같아야 로드됨

[permissions]
required = ["log"]              # 아직 강제되지 않음 (8번 참고)
optional = ["storage"]

[resources]
memory_limit = 16777216         # 현재 읽기만 하고 실제로 강제하지 않음 (8번 참고)
fuel_limit = 100000000          # 이건 실제로 wasmtime Store에 설정됨

[dependencies]                  # 파싱만 되고 의존성 해석 로직 없음 (8번 참고)
```

`api_version` 검증은 **2026-08-28에 새로 연결**했습니다
(`manifest::validate_api_version`, commit `acb569b`) — MAJOR가 다르면 컴파일
단계에서 거부되고, MINOR가 런타임보다 높으면 경고 로그만 남기고 허용됩니다.
그 전에는 이 필드가 파싱만 되고 전혀 검증되지 않았습니다.

## 7. 실전 예제 — `hello-world` 플러그인 해부

`plugins/hello-world/plugin.wat`는 손으로 쓴 WebAssembly Text 포맷입니다 (이
프로젝트에 아직 언어별 SDK가 없어서, 지금 존재하는 유일한 동작 예시는 이걸
직접 손으로 쓴 것입니다):

- `allocate_buffer`: 8바이트 정렬 bump allocator. 힙 포인터를 65536(첫 64KB
  이후)부터 시작해서 요청 크기만큼 전진시키기만 함 — **해제를 안 함**.
- `free_buffer`: `nop` — 아무것도 안 함. bump allocator라서 회수가 없음.
- 4개 lifecycle export: 전부 그냥 `0`(성공) 반환.
- `handle_command`/`handle_event`: 전달받은 인자를 무시하고 `0` 반환.

즉 `hello-world`는 "링크가 되는지, 생명주기가 도는지"만 확인하는 스캐폴드입니다
— 실제로 뭘 하지는 않습니다. 진짜 기능이 있는 플러그인을 만들려면 이 계약
위에 실제 로직을 얹어야 합니다.

## 8. 지금 플러그인을 만들려면 실제로 뭘 해야 하나

1. 위 계약(5번 섹션)을 만족하는 WASM 모듈을 만든다 — 언어는 상관없습니다.
   WASM으로 컴파일되고 이 9개를 export할 수 있으면 됩니다 (Rust →
   `wasm32-unknown-unknown`, AssemblyScript, 손으로 쓴 WAT 등).
2. `plugin_host` 모듈에서 14개 host function을 import한다.
3. `plugin.toml`을 작성한다. `api_version`은 `0.1.x`로 맞추세요 (현재
   `RUNTIME_API_VERSION`).
4. `plugins/<이름>/` 아래 `plugin.toml` + `plugin.wasm`을 둔다.

**아직 없는 것**: 언어별 SDK(코드 생성, 타입 바인딩, 버퍼 관리 자동화),
`permissions`/`memory_limit` 강제, 플러그인 간 `dependencies` 해석, 이벤트
구독/디스패치, 타이머. 전부 9번 섹션에 정리했습니다.

## 9. 설계 문서 vs 실제 구현 — 뭘 믿어야 하나

| 항목 | `docs/02-plugin/03-plugin-api-contract.md` | 실제 코드 |
|---|---|---|
| 버퍼 관리 방식 | `allocate_buffer(size)→buffer_id`, 별도 `read_buffer`/`write_buffer` host function | 그런 host function 없음. 포인터+길이를 직접 주고받음 (3번 섹션) |
| 타이머 (`set_timer`/`cancel_timer`/`handle_timer`) | 설계됨 | **미구현** — GitHub 이슈 [#23](https://github.com/endlessGold/the-protocol/issues/23) |
| 에러 코드 체계 (`-10`~`-59`, 카테고리별) | 상세히 설계됨 | 함수마다 즉흥적인 값 (`-1`, `-2`, `-20`, `-30`) |
| TypeScript/C# SDK 패키지 | `@the-protocol/sdk`, `TheProtocol.Sdk` 예시 코드 있음 | **존재 안 함** — `sdk/typescript/`, `sdk/csharp/` 빈 디렉토리, GitHub 이슈 [#20](https://github.com/endlessGold/the-protocol/issues/20) |
| `PlayerData` (Position/Stats/StatusEffect 포함) | 풍부한 구조 | 실제는 `{id, name, level, hp, max_hp, mp, max_mp, room_id}`만 |
| `combat_action`의 실제 전투 로직 | `CombatActionType` enum, 데미지 계산 | **스텁** — 턴 카운터만 증가. `application` 레이어의 진짜 전투 시스템과도 분리됨 — GitHub 이슈 [#24](https://github.com/endlessGold/the-protocol/issues/24) |
| `api_version` 호환성 검증 | 설계됨 | **2026-08-28에 연결함** (6번 섹션) |
| `permissions`/`memory_limit` 강제 | 암묵적으로 전제 | 매니페스트에서 읽기만 하고 강제 안 함 |
| `emit_event`의 구독/디스패치 | 이벤트 타입 15종 목록까지 설계 | 이벤트를 큐에 쌓기만 함, 아무도 읽지 않음 |

`docs/00-status/implementation-status.md`의 `core/plugin` 섹션도 이 실제
구현 이전 스냅샷을 설명하고 있어 stale합니다 — GitHub 이슈
[#25](https://github.com/endlessGold/the-protocol/issues/25).

**원칙**: `docs/02-plugin/`, `docs/design/` 아래 문서는 설계/계획 문서입니다.
실제 동작을 알고 싶으면 항상 `core/plugin/src/`를 직접 읽으세요. 이 문서는
그 코드를 읽고 쓴 것이고, 코드가 바뀌면 이 문서도 stale해집니다 — 날짜를
확인하세요.
