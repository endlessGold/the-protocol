# 프레젠테이션 커맨드 프로토콜

> 작성일: 2026-08-28
> 목적: 코어(러스트)가 게임 엔진(1차: Godot)을 "리모트 컨트롤"하기 위한 안정적이고
> 제네릭한 명령 어휘. 코어는 계속 진화하지만, 엔진 쪽이 구현하는 이 어휘는
> 거의 바뀌지 않아야 한다 — 그래야 코어가 바뀔 때마다 바인더까지 매번
> 갈아엎지 않아도 된다.

## 0. 설계 원칙

1. **제네릭할 것** — "레벨업 이펙트를 재생하라"가 아니라 "`play_effect(name="level_up", ...)`을 호출하라". 새 이펙트/스탯/UI가 늘어나도 이 함수들의 시그니처는 안 바뀐다.
2. **엔진은 "무엇을 보여줄지" 모른다, "무엇이 일어났는지"만 안다** — 렌더링/애니메이션/셰이더 파라미터 매핑은 전부 엔진(Godot) 쪽 책임. 코어는 절대 `Vector3`, `Node`, 셰이더 유니폼 이름 같은 걸 몰라야 한다.
3. **이 프로젝트는 방(room) 기반 MUD이지 좌표 기반 공간 게임이 아니다** — `domain/src/world.rs`의 `Room`/`Npc`에는 좌표가 없다. "이동"은 `room_id` 전이지 `Vector3` 이동이 아니다. 엔진이 각 room_id를 씬의 어디에 배치할지는 전적으로 엔진 쪽 결정.

## 1. 코어 → 엔진 (`PresentationCommand`)

| 커맨드 | 필드 | 대응하는 실제 도메인 개념 |
|---|---|---|
| `SpawnEntity` | `entity_id, kind, room_id, display_name` | 캐릭터/NPC가 씬에 나타남 |
| `DespawnEntity` | `entity_id` | 사망/퇴장 |
| `EnterRoom` | `entity_id, room_id` | `DomainEvent::PlayerEnteredRoom` |
| `LeaveRoom` | `entity_id, room_id` | `DomainEvent::PlayerLeftRoom` |
| `UpdateProperty` | `entity_id, key: String, value: PropertyValue` | hp/mp/level 등 — 새 스탯이 늘어도 이 커맨드는 안 바뀜 |
| `PlayEffect` | `name: String, entity_id: Option<u64>, params: HashMap<String, PropertyValue>` | 피격/레벨업/전투 시작 등. **셰이더/VFX 파라미터도 `params`로 전달** — 예: `params={"color": "#ff3333", "intensity": 0.8}`. 코어는 이 값이 셰이더 유니폼으로 매핑된다는 걸 몰라도 된다 |
| `ShowMessage` | `text: String, target_entity_id: Option<u64>` | 전투 로그, 채팅, 시스템 메시지 |
| `DrawUi` | `layout: UiLayout` (2단계 — 아래 §4 참고) | 인벤토리창, HP바 등 |

## 2. 엔진 → 코어 (`EngineInput`)

| 커맨드 | 필드 | 설명 |
|---|---|---|
| `Action` | `action: String, payload: HashMap<String, PropertyValue>` | 버튼 입력 등을 제네릭하게 (구체적인 "attack" 액션이 아니라 이름표만) |
| `Tick` | `delta_seconds: f64` | 프레임/고정 간격 시뮬레이션 진행 |

## 3. `PropertyValue` — 최소 타입 집합

```rust
pub enum PropertyValue {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
}
```
욕심내서 타입을 늘리지 않는다 — 셰이더 파라미터, 스탯, 문자열 라벨 전부 이 4종으로 표현 가능해야 이 계층이 계속 안정적으로 유지된다.

## 4. `DrawUi` (UiLayout) — 1차는 범위에서 제외

UI 레이아웃을 제네릭하게 서술하는 건 그 자체로 별도 설계가 필요한 큰 주제다
(패널/텍스트/버튼 트리 구조, 이벤트 콜백 등). 1차 구현에서는 `DrawUi`를
`ShowMessage`/`UpdateProperty` 조합으로 대체하고(HP바는 UpdateProperty로 값만
보내고 그리기는 Godot의 기존 UI 노드가 구독), 본격적인 `UiLayout` 스키마는
실제 화면이 몇 개 필요해지는 시점에 다시 설계한다. 지금 무리하게 스키마를
확정하면 추측성 설계가 된다.

## 5. `PresentationSink` 트레이트

```rust
pub trait PresentationSink {
    fn send(&self, command: PresentationCommand);
}
```

Godot 바인더가 이 트레이트를 구현해서 시그널로 재발행하게 되지만, **gdext
의존성이 전혀 없는 순수 러스트 트레이트**라서 테스트용 구현(커맨드를 그냥
Vec에 쌓는 것 등)도 쉽게 만들 수 있다. 바인더가 아직 없어도 이 계층은 지금
바로 만들고 테스트할 수 있다.

## 6. 엔티티 모델 통합 — 완료 (2026-08-28)

원래 여기엔 "이벤트가 아직 안 나온다"는 문제가 적혀 있었다. 원인과 수정 내역을
기록으로 남긴다.

`DomainEvent`를 실제로 만들어서 반환하는 곳은 코드 전체에서 **딱 두 곳**이었다
(`domain::Character::gain_experience()` → `LevelUp`, `domain::Combat::
process_attack()` → `AttackExecuted`/(죽으면) `LevelUp`+`CombatEnded`). 그런데
**`application::GameWorld::start_combat()`(실제 네트워크 커맨드가 타는 경로)은
`process_attack()`을 호출하지 않고** 자기만의 단순 데미지 계산을 따로 했다 —
이벤트를 하나도 만들지 않았다.

원인: `process_attack(attacker: &mut Character, target: &mut Character)`은
양쪽 다 `Character` 타입이어야 했는데, NPC는 `Npc`라는 별개 타입이었다(스탯도
`take_damage()`도 없음). `start_combat()`은 이 때문에 NPC 데미지 계산을 위해
"가짜 Character를 즉석에서 만드는" 우회를 쓰고 있었다(모든 NPC가 위협 수준과
무관하게 똑같은 `Stats{5,5,5,5,10}`을 가짐).

**수정**: `domain::Combatant` 트레이트를 추가하고(`domain/src/combatant.rs`)
`Character`/`Npc` 둘 다 구현하게 했다. `Npc`에 `level`/`attack`/`defense`
필드를 새로 추가하고(기존 4개 NPC에 설명에 맞는 실제 값 부여 — 마을 경비병은
강하게, 고블린은 약하게), `Combat::calculate_damage`/`process_attack`을
`&(mut) dyn Combatant`를 받도록 일반화했다. `start_combat()`은 이제
`combat.process_attack(&mut character, &mut npc)`을 그대로 호출한다 — 가짜
Character 우회 삭제. `create_character()`/`move_character()`도 각각
`CharacterCreated`/`PlayerEnteredRoom`+`PlayerLeftRoom`을 내보내도록 고쳤다
(전에는 그 이벤트들조차 어디서도 발행되지 않았다).

`GameWorld`에 `pending_events: Vec<DomainEvent>` 큐 + `drain_events()`를
추가했다 — 상태를 바꾸는 메서드(`create_character`/`move_character`/
`start_combat`)는 이제 이벤트를 큐에 쌓고, 호출자가 작업이 끝난 뒤
`drain_events()`로 걷어간다.

**엔티티 ID 충돌 주의**: `Character`와 `Npc`는 이제 하나의 프레젠테이션
`entity_id`(u64) 공간을 공유한다. NPC는 `World::initialize()`에서 정적으로
1~4번을 쓰므로, `GameWorld.next_character_id`의 시작값을 1000으로 올려서
당장의 충돌은 피했다 — 다만 이건 임시방편이다. NPC가 나중에 런타임에 동적으로
생성되면 진짜 공유 ID 할당자가 필요하다.

**아직 손 안 댐**: `core/plugin`의 `PlayerData`/`CombatState`는 이 통합과
완전히 별개다 — `core/plugin`은 `protocol_domain`/`protocol_application`에
의존하지 않는 독립된 섬이라, 거기까지 통합하는 건 훨씬 큰 별도 작업이다.

## 7. 네트워크 배선 — 멀티플레이어

Godot 클라이언트가 임베디드(FFI)로만 동작하는 게 아니라 **일반 멀티플레이어
클라이언트로도 접속할 수 있어야 한다**는 결정에 따라, 기존 TCP 프로토콜 위에
프레젠테이션 커맨드를 실어 보내는 배선을 추가했다.

**포맷 재사용**: 새 `MessageType`을 만들지 않고 기존 `MessageType::Event` +
`Event { id, event_type, timestamp, source, payload, targets }`를 그대로
썼다 — `payload`는 `Vec<PresentationCommand>`를 `rmp_serde`로 직렬화한
바이트, `event_type = "presentation_batch"`로 태그. `protocol-protocol`이
`protocol-presentation`을 몰라도 되게(불투명한 바이트로만 다룸) 하기 위한
선택이다.

**배선 지점**: `core/runtime/src/main.rs`의 `dispatch_events()` 헬퍼 —
`Vec<DomainEvent>`를 받아 `translate_event()`로 펼치고, 직렬화해서
`Event`로 감싸고, `SessionManager::broadcast()`로 보낸다. `MoveHandler`/
`AttackHandler`/`CreateCharacterHandler`가 각자 `GameWorld` 호출 후
`drain_events()` → `dispatch_events()` 순으로 부른다.

**지금은 방(room) 단위로 타게팅 안 함 — 전체 브로드캐스트**: 어떤 세션이 어느
방에 있는 플레이어인지(session_id → player_id → Character.room_id)를
교차 조회하는 로직이 아직 없어서, 일단 연결된 모든 세션에 전부 보낸다.
클라이언트가 자기 방과 무관한 갱신을 받아도 무시하면 되니 틀린 동작은
아니지만, 최적은 아니다 — 후속 이슈로 등록.

### 7.1 참고: 이 경로는 사실 gdext 없이도 갈 수 있다

§7의 배선(TCP + MessagePack `Event`)은 순수 GDScript TCP 클라이언트로도 받을
수 있다 — `bindings/godot`(gdext FFI)가 없어도 멀티플레이어 접속 자체는
가능하다는 뜻이다. 다만 **Godot 4 GDScript에는 MessagePack 코덱이 내장되어
있지 않다** — 손으로 구현하려면(정수 크기별 태그, 문자열/배열/맵 인코딩 등)
컴파일해서 검증할 방법 없이 짜기엔 꽤 위험한 분량이라, 이번 패스에서는
시도하지 않았다. 대안은: (a) GDScript MessagePack 코덱을 직접 짜거나, (b) 이
채널만 GDScript에 내장된 `JSON`으로 바꾸거나(다른 프로토콜 메시지는 그대로
MessagePack 유지), (c) `bindings/godot`가 준비될 때까지 기다려서 역직렬화를
러스트 쪽에서 끝내고 시그널만 던지게 하거나. 후속 이슈로 등록.

## 8. 참고 이슈

- ~~엔티티 모델 통일 (Npc/Character)~~: the-protocol #26 — 완료 (§6)
- ~~`GameWorld::start_combat()`이 `Combat::process_attack()`을 안 씀~~: the-protocol #27 — 완료 (§6)
- `DomainEvent` 디스패처: the-protocol #28 — §6/§7로 대체 완료
- 방(room) 단위 브로드캐스트 타게팅: 신규 등록 예정 (§7)
- `bindings/godot` 크레이트: the-protocol #29 — 여전히 gdext API 실검증 대기 중
