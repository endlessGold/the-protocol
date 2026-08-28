# 전투 시스템 상세 설계

> 모듈: `domain::combat`
> 소스: `domain/src/combat.rs`

---

## 1. 전투 시스템 아키텍처

### 1.1 전체 구조도

```
┌─────────────────────────────────────────────────┐
│                  Application Layer               │
│  GameWorld::start_combat() → 전투 시작            │
│  GameWorld::process_turn() → 턴 처리              │
└──────────────────────┬──────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────┐
│                  Domain Layer                    │
│  Combat 엔티티                                    │
│  ├─ calculate_damage()   데미지 계산               │
│  ├─ process_attack()     공격 처리                 │
│  └─ combat log           전투 로그                 │
│  DomainEvent 발생                                │
│  ├─ CombatStarted                                  │
│  ├─ AttackExecuted                                 │
│  └─ CombatEnded                                    │
└──────────────────────┬──────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────┐
│                  Domain Events                   │
│  이벤트 버스를 통해 상태 동기화                    │
└─────────────────────────────────────────────────┘
```

### 1.2 Combat 엔티티

```rust
pub struct Combat {
    pub id: u64,              // 전투 고유 ID
    pub attacker_id: u64,     // 공격자 캐릭터 ID
    pub target_id: u64,       // 대상 NPC ID
    pub state: CombatState,   // 전투 상태
    pub turn: u32,            // 현재 턴 번호
    pub log: Vec<CombatAction>, // 전투 로그
}

pub enum CombatState {
    InProgress,                    // 전투 진행 중
    Finished { winner_id: u64 },  // 전투 종료 + 승자
}
```

---

## 2. 턴 기반 전투 설계

### 2.1 전투 흐름

```
1. 전투 시작
   ├─ Combat::new() 호출
   ├─ CombatStarted 이벤트 발생
   └─ turn = 1

2. 턴 반복
   ├─ 플레이어 공격 (현재: 1회 공격만 처리)
   ├─ 데미지 계산
   ├─ 데미지 적용
   ├─ 사망 판정
   │   ├─ 사망 시 → CombatEnded 이벤트, 전투 종료
   │   └─ 생존 시 → 다음 턴 진행
   └─ turn += 1

3. 전투 종료
   ├─ CombatState::Finished { winner_id }
   ├─ 승자에게 경험치 보상
   └─ 레벨업 처리 (LevelUp 이벤트)
```

### 2.2 현재 구현의 전투 모델

현재 구현에서는 **단일 턴 공격**만 지원:

```rust
// application/src/service.rs:161-228
pub fn start_combat(...) -> Result<CombatInfo, ApplicationError> {
    // 1. 공격자/대상 검증
    // 2. Combat 엔티티 생성
    // 3. 단일 데미지 계산
    // 4. 대상 HP 감소
    // 5. 결과 반환
}
```

**한계점:**
- 플레이어만 공격 가능 (NPC 공격 로직 없음)
- 턴 반복 없음 (한 번 공격 후 전투 종료)
- 방어/스킬 선택 불가

---

## 3. 데미지 계산 공식

### 3.1 현재 구현

```rust
pub fn calculate_damage(attacker: &Character, target: &Character) -> u32 {
    let base_damage = attacker.stats.strength as f64;
    let defense = target.stats.constitution as f64 * 0.5;
    let raw_damage = (base_damage - defense).max(1.0);  // 최소 데미지 1

    let mut rng = rand::thread_rng();
    let variance = raw_damage * 0.2;  // ±20% 변동
    let final_damage = raw_damage + rng.gen_range(-variance..variance);
    final_damage.max(1.0) as u32
}
```

### 3.2 데미지 공식 분해

```
최종 데미지 = max(1, (STR - CON×0.5) × (1 ± 0.2))
```

| 단계 | 공식 | 비고 |
|------|------|------|
| 기본 데미지 | `base_damage = STR` | 공격자 근력 |
| 방어력 | `defense = CON × 0.5` | 방어 시 50% 효율 |
| 원시 데미지 | `raw = max(1, base - defense)` | 최소 1 데미지 |
| 변동폭 | `variance = raw × 0.2` | ±20% 랜덤 |
| 최종 데미지 | `final = max(1, raw + random)` | 최소 1 보장 |

### 3.3 데미지 예시

| 공격자 STR | 대상 CON | 원시 데미지 | 변동 범위 | 실제 데미지 범위 |
|------------|----------|-------------|-----------|-----------------|
| 15 (Warrior) | 10 | 10.0 | ±2.0 | 8~12 |
| 15 (Warrior) | 14 (Warrior) | 8.0 | ±1.6 | 6~10 |
| 8 (Mage) | 10 | 3.0 | ±0.6 | 2~4 |
| 10 (Rogue) | 12 | 4.0 | ±0.8 | 3~5 |

### 3.4 확장 데미지 공식 (설계)

```rust
// 마법 데미지 (미구현)
pub fn calculate_magic_damage(attacker: &Character, target: &Character) -> u32 {
    let base_damage = attacker.stats.intelligence as f64 * 1.5;
    let magic_defense = target.stats.wisdom as f64 * 0.3;
    let raw_damage = (base_damage - magic_defense).max(1.0);

    let variance = raw_damage * 0.2;
    let mut rng = rand::thread_rng();
    let final_damage = raw_damage + rng.gen_range(-variance..variance);
    final_damage.max(1.0) as u32
}
```

---

## 4. 명중률 계산 (설계)

### 4.1 명중률 공식

```
명중률 = base_hit_rate + (DEX × hit_per_dex) - target_dodge
```

| 파라미터 | 기본값 | 비고 |
|----------|--------|------|
| `base_hit_rate` | 85% | 모든 캐릭터 기본 명중률 |
| `hit_per_dex` | +1% | DEX 1당 명중률 증가 |
| `target_dodge` | 회피율 | 대상의 회피율 차감 |

### 4.2 명중 판정

```rust
pub fn check_hit(attacker: &Character, target: &Character) -> bool {
    let hit_chance = 0.85 + (attacker.stats.dexterity as f64 * 0.01);
    let dodge_chance = calculate_dodge_rate(target);
    let final_hit = (hit_chance - dodge_chance).max(0.05).min(0.95);

    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < final_hit
}
```

---

## 5. 회피율 계산 (설계)

### 5.1 회피율 공식

```
회피율 = DEX × 2% (최대 40%)
```

| DEX | 회피율 | 비고 |
|-----|--------|------|
| 8 | 16% | Cleric/Mage |
| 10 | 20% | 기본값 |
| 12 | 24% | Rogue |
| 15 | 30% | Rogue (클래스 보너스) |
| 20+ | 40% (cap) | 상한선 |

### 5.2 회피 시 처리

```rust
pub fn check_dodge(target: &Character) -> bool {
    let dodge_rate = (target.stats.dexterity as f64 * 0.02).min(0.40);
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < dodge_rate
}
```

회피 성공 시:
- 데미지 0 적용
- `CombatAction` 로그에 "회피" 메시지 기록
- 경험치 보상 없음

---

## 6. 치명타 시스템 (설계)

### 6.1 치명타 확률

```
치명타 확률 = base_crit + (DEX × crit_per_dex)
치명타 배율 = 1.5x ~ 2.0x (클래스별 차등)
```

| 클래스 | 기본 치명타 | DEX 보너스 | 치명타 배율 |
|--------|------------|------------|------------|
| Warrior | 5% | +0.5%/DEX | 1.5x |
| Mage | 3% | +0.3%/DEX | 2.0x |
| Rogue | 10% | +1.0%/DEX | 1.8x |
| Cleric | 5% | +0.5%/DEX | 1.5x |

### 6.2 치명타 판정

```rust
pub fn check_critical(attacker: &Character) -> bool {
    let base_crit = match attacker.class {
        CharacterClass::Warrior => 0.05,
        CharacterClass::Mage => 0.03,
        CharacterClass::Rogue => 0.10,
        CharacterClass::Cleric => 0.05,
    };
    let crit_per_dex = match attacker.class {
        CharacterClass::Rogue => 0.01,
        _ => 0.005,
    };
    let crit_chance = base_crit + (attacker.stats.dexterity as f64 * crit_per_dex);
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < crit_chance.min(0.50)  // 최대 50%
}
```

---

## 7. 방어/디펜드 메커니즘

### 7.1 방어 시스템

현재 `CombatActionType::Defend`는 열거형에 존재하지만 실제 구현 없음.

```rust
pub enum CombatActionType {
    Attack,
    Defend,  // 미구현
}
```

### 7.2 방어 설계

```rust
pub fn process_defend(&mut self, defender: &mut Character) -> Vec<DomainEvent> {
    // 1. 방어 상태 플래그 설정
    self.defending = Some(defender.id);

    // 2. 다음 받는 데미지 50% 감소
    // 3. 방어 턴 동안 공격 불가
    // 4. 방어 성공 시 MP 5 회복 (보상)

    vec![DomainEvent::DefendActivated {
        combat_id: self.id,
        defender_id: defender.id,
    }]
}
```

### 7.3 방어 효과

| 효과 | 배율 | 비고 |
|------|------|------|
| 받는 데미지 | 50% 감소 | 방어 턴 동안 |
| 공격 가능 | ❌ | 방어 중 공격 불가 |
| MP 회복 | +5 | 방어 성공 보상 |
| 회피 불가 | ❌ | 방어 중 회피 불가 (방어 자체가 회피 대체) |

---

## 8. 도망 시스템 (설계)

### 8.1 도망 확률

```
도망 확률 = 50% + (DEX × 2%) - (target_level × 3%)
```

### 8.2 도망 처리

```rust
pub fn attempt_flee(player: &Character, target_level: u32) -> bool {
    let base_rate = 0.50;
    let dex_bonus = player.stats.dexterity as f64 * 0.02;
    let level_penalty = target_level as f64 * 0.03;
    let flee_chance = (base_rate + dex_bonus - level_penalty).max(0.10).min(0.90);

    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < flee_chance
}
```

### 8.3 도망 시 패널티

| 패널티 | 효과 | 비고 |
|--------|------|------|
| 경험치 손실 | 현재 턴 획득 XP 없음 | |
| 전투 로그 | "도망쳤다" 메시지 기록 | |
| 방어력 감소 | 다음 전투 -10% 방어력 | 3턴간 지속 |

---

## 9. 전투 로그 구조

### 9.1 CombatAction

```rust
pub struct CombatAction {
    pub actor_id: u64,        // 행동한 주체 ID
    pub action_type: CombatActionType, // 행동 유형
    pub damage: Option<u32>,  // 입힌 데미지 (방어/회피 시 None)
    pub message: String,      // 사람-readable 메시지
}

pub enum CombatActionType {
    Attack,   // 공격
    Defend,   // 방어 (미구현)
    // 향후 확장: Skill, Flee, Heal 등
}
```

### 9.2 로그 메시지 포맷

```
"[attacker] hits [target] for [damage] damage!"
"[target] dodges the attack!"
"[attacker] lands a critical hit for [damage] damage!"
"[defender] takes a defensive stance."
"[actor] attempts to flee... success!"
```

### 9.3 로그 저장 위치

- `Combat.log: Vec<CombatAction>` — Combat 엔티티에 내장
- 전투 종료 후에도 유지 (전투 기록 조회용)
- 향후: 별도 전투 기록 테이블로 분리 가능

---

## 10. 사망 처리

### 10.1 NPC 사망

```rust
// combat.rs:85-98
if !target.is_alive() {
    self.state = CombatState::Finished {
        winner_id: self.attacker_id,
    };
    // 경험치 보상
    let xp = 100 * target.level as u64;
    let level_events = attacker.gain_experience(xp);
    events.extend(level_events);
    // CombatEnded 이벤트 발생
}
```

### 10.2 캐릭터 사망 (설계)

현재 구현에서 캐릭터 사망 처리 없음. 설계:

```rust
if !attacker.is_alive() {
    self.state = CombatState::Finished {
        winner_id: self.target_id,
    };

    // 사망 패널티
    let xp_loss = attacker.experience / 10;  // 10% 경험치 손실
    attacker.experience = attacker.experience.saturating_sub(xp_loss);

    // 부활: 시작 방으로 이동, HP 50% 회복
    attacker.room_id = 1;  // Town Square
    attacker.hp = attacker.max_hp / 2;

    events.push(DomainEvent::CharacterDied {
        character_id: attacker.id,
        xp_loss,
    });
}
```

### 10.3 경험치 보상 공식

```
보상 XP = 100 × 대상 레벨
```

| 대상 레벨 | 보상 XP | 다음 레벨 필요 XP (대상 기준) |
|-----------|---------|-------------------------------|
| 1 | 100 | 1,000 |
| 5 | 500 | 5,000 |
| 10 | 1,000 | 10,000 |
| 20 | 2,000 | 20,000 |

---

## 11. 전투 보상

### 11.1 경험치 보상

- **PvE (NPC 처치)**: `100 × target.level`
- **연속 처치 보너스**: 미구현 (설계: 연속 3마리 이상 +50%)
- **레벨 차이 보정**: 미구현 (설계: `× (target_level / player_level)`)

### 11.2 전리품 (Loot)

현재 구현 없음. 설계:

```rust
pub struct LootTable {
    pub drops: Vec<LootEntry>,
}

pub struct LootEntry {
    pub item_id: u32,
    pub drop_rate: f64,      // 0.0 ~ 1.0
    pub min_quantity: u32,
    pub max_quantity: u32,
}
```

---

## 12. PvP vs PvE 차이점

### 12.1 PvE (Player vs Environment)

| 항목 | PvE |
|------|-----|
| 대상 | NPC |
| AI | 없음 (단순 HP 감소) |
| 데미지 공식 | `STR - CON×0.5` (현재) |
| 보상 | 경험치 + 전리품 |
| 사망 패널티 | 경험치 10% 손실 |

### 12.2 PvP (Player vs Player) — 미구현

| 항목 | PvP |
|------|-----|
| 대상 | 다른 플레이어 |
| 매칭 | 같은 방에 있는 플레이어 |
| 데미지 공식 | 클래스별 보정 적용 |
| 보상 | 승자에게 패자 경험치 5% |
| 사망 패널티 | 경험치 20% 손실 + 5분 부활 대기 |
| 전투 참가 인원 | 1:1 기본, 향후 1:1 / N:전체 |

---

## 13. 현재 구현 vs 미구현

### ✅ 구현 완료

| 기능 | 위치 | 상태 |
|------|------|------|
| Combat 엔티티 생성 | `Combat::new()` | ✅ 완료 |
| 데미지 계산 공식 | `Combat::calculate_damage()` | ✅ 완료 |
| 공격 처리 (단일 턴) | `Combat::process_attack()` | ✅ 완료 |
| 데미지 적용 | `Character::take_damage()` | ✅ 완료 |
| 사망 판정 | `Character::is_alive()` | ✅ 완료 |
| 전투 종료 판정 | `CombatState::Finished` | ✅ 완료 |
| 경험치 보상 | `gain_experience()` | ✅ 완료 |
| 전투 로그 기록 | `Combat.log` | ✅ 완료 |
| DomainEvent 발생 | `CombatStarted/AttackExecuted/CombatEnded` | ✅ 완료 |

### ❌ 미구현

| 기능 | 우선순위 | 예상 작업량 |
|------|----------|-------------|
| 턴 반복 전투 | 🔴 높음 | Medium |
| NPC 반격 로직 | 🔴 높음 | Medium |
| 방어(Defend) 메커니즘 | 🟡 중간 | Small |
| 명중/회피/치명타 | 🟡 중간 | Medium |
| 마법 데미지 시스템 | 🟡 중간 | Medium |
| 도망 시스템 | 🟢 낮음 | Small |
| PvP 전투 | 🟢 낮음 | Large |
| 전투 보상 (전리품) | 🟡 중간 | Medium |
| 데미지 타입 분리 (물리/마법) | 🟢 낮음 | Medium |

---

## 14. 확장 고려사항

### 14.1 이벤트 기반 전투 시스템

현재 `process_attack()`이 직접 상태를 변경하나, 향후 이벤트 기반으로 전환 가능:

```rust
// 이벤트 기반 전환 설계
pub fn handle_combat_event(event: DomainEvent) -> Vec<DomainEvent> {
    match event {
        DomainEvent::AttackRequested { attacker_id, target_id } => {
            // 데미지 계산 → AttackExecuted 이벤트 반환
        }
        DomainEvent::DefendRequested { defender_id } => {
            // 방어 상태 설정 → DefendActivated 이벤트 반환
        }
        _ => vec![],
    }
}
```

### 14.2 버프/디버프 시스템

전투 중 적용되는 지속 효과:
- 독: 매턴 5 데미지
- 화상: 매턴 3 데미지
- 빙결: DEX 50% 감소
- 출혈: 매턴 2 데미지, 이동 불가
