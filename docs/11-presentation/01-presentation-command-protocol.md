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

## 6. 정직하게 짚어야 할 문제 — 이벤트가 아직 안 나온다

`DomainEvent`를 실제로 만들어서 반환하는 곳은 코드 전체에서 **딱 두 곳**이다:

- `domain::Character::gain_experience()` → `LevelUp`
- `domain::Combat::process_attack()` → `AttackExecuted`, (죽으면) `LevelUp` + `CombatEnded`

그런데 **`application::GameWorld::start_combat()`(실제로 네트워크 커맨드가 호출하는 경로)은 `process_attack()`을 호출하지 않는다.** 대신 자기만의 단순한 데미지 계산을 따로 하고, 이벤트를 하나도 만들지 않는다.

왜 이렇게 됐는지도 확인했다 — **`process_attack(attacker: &mut Character, target: &mut Character)`은 양쪽 다 `Character` 타입이어야 하는데, NPC는 `Npc`라는 별개 타입이다** (스탯도, `take_damage()`도, `gain_experience()`도 없음). 그래서 `start_combat()`은 NPC 데미지 계산을 위해 "가짜 Character를 즉석에서 만드는" 우회를 쓰고 있다 (known-issues #13). `process_attack()`을 그대로 쓰려면 `Npc`가 최소한 데미지를 받고 확인할 수 있는 공통 인터페이스가 있어야 한다 — 이게 바로 지난 대화에서 나온 "엔티티 모델 통일"의 구체적인 실체다.

**결론**: 프레젠테이션 커맨드 프로토콜(이 문서, §1~5)은 지금 바로 만들고 테스트할 수 있다 — `DomainEvent`가 실제로 얼마나 나오는지와 무관하게 독립적인 계층이다. 다만 **실제로 전투 중 `AttackExecuted`/`CombatEnded`가 흘러나오게 하려면 `Npc`/`Character` 통합이 선행되어야 한다.** 이 순서를 바꿔서 이벤트 배관부터 서두르면, 결국 지금의 "가짜 Character" 우회를 프레젠테이션 계층까지 끌고 올라가게 된다.

## 7. 참고 이슈

- 엔티티 모델 통일 (Npc/Character): 신규 등록 예정
- `GameWorld::start_combat()`이 `Combat::process_attack()`을 안 씀: 신규 등록 예정
- 도메인 이벤트 디스패처 부재: known-issues #17 (이 문서의 §6이 그 구체적인 원인 분석)
