# 캐릭터 시스템 상세 설계

> 모듈: `domain::character`
> 소스: `domain/src/character.rs`

---

## 1. 캐릭터 엔티티 전체 구조

### 1.1 Character 구조체

```rust
pub struct Character {
    pub id: u64,              // 고유 식별자 (application 레이어에서 할당)
    pub name: String,         // 캐릭터 이름 (유니크 제약)
    pub class: CharacterClass, // 클래스 종류
    pub level: u32,           // 현재 레벨 (최소 1)
    pub experience: u64,      // 누적 경험치
    pub hp: u32,              // 현재 HP
    pub max_hp: u32,          // 최대 HP
    pub mp: u32,              // 현재 MP
    pub max_mp: u32,          // 최대 MP
    pub stats: Stats,         // 기본 스탯
    pub room_id: u32,         // 현재 위치 (Room ID 참조)
    pub inventory: Inventory, // 인벤토리 (내장)
}
```

### 1.2 필드별 제약조건

| 필드 | 타입 | 제약조건 | 비고 |
|------|------|----------|------|
| `id` | `u64` | `> 0`, 유니크 | application에서 시퀀스 할당 |
| `name` | `String` | 유니크, 1~32자 | 공백/특수문자 제한 (application 검증) |
| `class` | `CharacterClass` | enum 4값 | Warrior, Mage, Rogue, Cleric |
| `level` | `u32` | `≥ 1`, `≤ 100` (설계) | 현재 검증 없음 |
| `experience` | `u64` | `≥ 0` | 누적형, 오버플로우 주의 |
| `hp` | `u32` | `0 ≤ hp ≤ max_hp` | take_damage/heal에서 자동 보정 |
| `max_hp` | `u32` | `> 0` | 레벨업 시 +10 |
| `mp` | `u32` | `0 ≤ mp ≤ max_mp` | 회복 메커니즘 미구현 |
| `max_mp` | `u32` | `> 0` | 생성 시 고정 |
| `stats` | `Stats` | 각 필드 `u32` | 클래스별 기본값 다름 |
| `room_id` | `u32` | Room 테이블 FK | 이동 시 갱신 |
| `inventory` | `Inventory` | 용량 20 (기본) | 아이템 스택 |

---

## 2. 캐릭터 클래스 시스템

### 2.1 클래스 열거형

```rust
pub enum CharacterClass {
    Warrior,
    Mage,
    Rogue,
    Cleric,
}
```

### 2.2 각 클래스별 기본 스탯

| 클래스 | STR | DEX | INT | WIS | CON | 설계 의도 |
|--------|-----|-----|-----|-----|-----|-----------|
| **Warrior** | 15 | 10 | 8 | 8 | 14 | 근접 물리 딜러, 탱커 |
| **Mage** | 8 | 10 | 15 | 12 | 10 | 원거리 마법 딜러, 유틸리티 |
| **Rogue** | 10 | 15 | 10 | 8 | 12 | 기습/도둑, 높은 회피 |
| **Cleric** | 10 | 8 | 12 | 15 | 12 | 힐러, 버프, 방어 |

**스탯 합산 비교:**
- Warrior: 15+10+8+8+14 = **55**
- Mage: 8+10+15+12+10 = **55**
- Rogue: 10+15+10+8+12 = **55**
- Cleric: 10+8+12+15+12 = **57** (+Cleric은 2점 많음 — 힐러 밸런스 보정)

### 2.3 클래스 고유 능력 (설계)

| 클래스 | 능력명 | 효과 | MP 소모 | 쿨다운 | 비고 |
|--------|--------|------|---------|--------|------|
| **Warrior** | 강타 (Heavy Strike) | `damage = STR * 2.0` | 10 | 3턴 | 단일 타겟 강공격 |
| **Warrior** | 방어 태세 (Guard Stance) | 1턴 방어 시 받는 데미지 50% 감소 | 5 | 2턴 | 방어력 배율 증가 |
| **Mage** | 화염 폭발 (Fireball) | `damage = INT * 1.5` | 20 | 4턴 | 광역 공격 (모든 적) |
| **Mage** | 얼음 방벽 (Ice Shield) | HP 대신 MP로 데미지 흡수 | 15 | 5턴 | MP→HP 전환 방어 |
| **Rogue** | 기습 (Ambush) | 회피 성공 시 다음 공격 치명타 보장 | 8 | 3턴 | 연계 시너지 |
| **Rogue** | 독 칼날 (Poison Blade) | 3턴간 매턴 5 damage DOT | 12 | 6턴 | 지속 데미지 |
| **Cleric** | 치유의 빛 (Heal Light) | `heal = WIS * 1.5` | 15 | 3턴 | 아군 HP 회복 |
| **Cleric** | 신의 축복 (Divine Blessing) | 아군 전체 공격력 +20% 3턴 | 25 | 8턴 | 버프 스킬 |

### 2.4 밸런스 설계 원칙

1. **스탯 총합 동일**: 모든 클래스의 기본 스탯 합이 동일 (55점)
2. **핵심 스탯 차별화**: 각 클래스의 핵심 스탯이 15로 동일, 보조 스탯 차이로 플레이 스타일 구분
3. **HP/MP 트레이드오프**: Warrior는 높은 HP/낮은 MP, Mage는 반대
4. **도전적 성장**: 레벨업 시 고정 성장 (+10 HP)으로 밸런스 유지
5. **클래스 간 상성**: Warrior > Rogue > Mage > Cleric > Warrior (가위바위보 구조)

---

## 3. 레벨 시스템

### 3.1 경험치 공식

```rust
pub fn xp_for_next_level(&self) -> u64 {
    (self.level as u64) * 1000
}
```

| 레벨 | 필요 XP | 누적 XP | 비고 |
|------|---------|---------|------|
| 1 → 2 | 1,000 | 1,000 | 첫 레벨업 |
| 2 → 3 | 2,000 | 3,000 | |
| 5 → 6 | 5,000 | 15,000 | |
| 10 → 11 | 10,000 | 55,000 | |
| 20 → 21 | 20,000 | 210,000 | |
| 50 → 51 | 50,000 | 1,275,000 | |
| 99 → 100 | 99,000 | 4,950,000 | 최대 레벨 |

### 3.2 레벨업 시 스탯 성장

```rust
// 현재 구현 (character.rs:130)
self.max_hp += 10;
self.hp = self.max_hp;  // 레벨업 시 HP 풀 회복
```

| 성장 항목 | 증가량 | 비고 |
|-----------|--------|------|
| max_hp | +10 | 고정 증가 |
| max_mp | +0 | **미구현** |
| STR/DEX/INT/WIS/CON | +0 | **미구현** |

**미구현 상세 설계:**

```rust
// 향후 설계안
pub fn level_up(&mut self) {
    self.level += 1;
    self.max_hp += 10;
    self.max_mp += 5;

    match self.class {
        CharacterClass::Warrior => {
            self.stats.strength += 3;
            self.stats.constitution += 2;
            self.stats.dexterity += 1;
        }
        CharacterClass::Mage => {
            self.stats.intelligence += 3;
            self.stats.wisdom += 2;
            self.stats.constitution += 1;
        }
        CharacterClass::Rogue => {
            self.stats.dexterity += 3;
            self.stats.strength += 1;
            self.stats.intelligence += 2;
        }
        CharacterClass::Cleric => {
            self.stats.wisdom += 3;
            self.stats.constitution += 2;
            self.stats.intelligence += 1;
        }
    }

    self.hp = self.max_hp;
    self.mp = self.max_mp;
}
```

### 3.3 최대 레벨

- **설정값**: 100레벨
- **제약조건**: 현재 코드에서 명시적 검증 없음
- **구현 필요**: `gain_experience` 내 `if self.level >= MAX_LEVEL { return; }` 체크

---

## 4. HP/MP 시스템

### 4.1 HP 계산

```rust
// 생성 시 (character.rs:86)
let max_hp = 50 + (base_stats.constitution * 2);
```

| 클래스 | CON | HP 공식 | 최대 HP |
|--------|-----|---------|---------|
| Warrior | 14 | 50 + 14×2 | **78** |
| Mage | 10 | 50 + 10×2 | **70** |
| Rogue | 12 | 50 + 12×2 | **74** |
| Cleric | 12 | 50 + 12×2 | **74** |

### 4.2 MP 계산

```rust
// 생성 시 (character.rs:95)
mp: 20 + (base_stats.wisdom),
```

| 클래스 | WIS | MP 공식 | 최대 MP |
|--------|-----|---------|---------|
| Warrior | 8 | 20 + 8 | **28** |
| Mage | 12 | 20 + 12 | **32** |
| Rogue | 8 | 20 + 8 | **28** |
| Cleric | 15 | 20 + 15 | **35** |

### 4.3 데미지 처리

```rust
pub fn take_damage(&mut self, amount: u32) -> u32 {
    let actual = amount.min(self.hp);  // HP 이하로 감소하지 않도록 보정
    self.hp -= actual;
    actual  // 실제 입힌 데미지 반환
}
```

### 4.4 HP 회복

```rust
pub fn heal(&mut self, amount: u32) -> u32 {
    let actual = amount.min(self.max_hp - self.hp);  // 최대 HP 초과 회복 방지
    self.hp += actual;
    actual  // 실제 회복량 반환
}
```

### 4.5 사망 판정

```rust
pub fn is_alive(&self) -> bool {
    self.hp > 0
}
```

### 4.6 회복 메커니즘 (현재 vs 설계)

| 메커니즘 | 상태 | 비고 |
|----------|------|------|
| 자연 회복 (시간 경과) | ❌ 미구현 | 매 턴마다 HP/MP 일정량 회복 |
| 포션 사용 | ❌ 미구현 | Consumable 아이템 사용 시 |
| 성소 회복 | ❌ 미구현 | 특정 방에서 휴식 시 |
| 레벨업 회복 | ✅ 구현 | max_hp 풀 회복 |
| MP 회복 | ❌ 미구현 | 모든 MP 관련 메커니즘 미구현 |

---

## 5. 스탯 시스템

### 5.1 스탯 정의

```rust
pub struct Stats {
    pub strength: u32,     // 근력
    pub dexterity: u32,    // 민첩
    pub intelligence: u32, // 지능
    pub wisdom: u32,       // 지혜
    pub constitution: u32, // 체질
}
```

### 5.2 각 스탯이 게임에 미치는 영향

| 스탯 | 영향 메커니즘 | 공식 | 비고 |
|------|--------------|------|------|
| **STR** | 물리 데미지 | `base_damage = STR` (combat.rs:51) | 공격력의 핵심 |
| **DEX** | 회피율, 명중률 | `회피율 = DEX × 2%` (설계) | 미구현 |
| **INT** | 마법 데미지 | `magic_damage = INT × 1.5` (설계) | 미구현 |
| **WIS** | MP, 힐량, 마법 저항 | `max_mp = 20 + WIS` | MP에만 영향 |
| **CON** | HP, 방어력 | `max_hp = 50 + CON×2`, `defense = CON×0.5` | HP/방어 모두 |

### 5.3 스탯 보정 공식

현재 구현에서 스탯은 **직접적 영향**만 미침. 향후 장비/버프 보정 설계:

```rust
// 장비 보정 포함 최종 스탯 계산 (설계)
pub fn effective_stats(&self) -> Stats {
    let mut base = self.stats.clone();

    // 장비 보정
    for equipment in &self.equipment {
        base.strength += equipment.bonus_strength;
        base.dexterity += equipment.bonus_dexterity;
        base.intelligence += equipment.bonus_intelligence;
        base.wisdom += equipment.bonus_wisdom;
        base.constitution += equipment.bonus_constitution;
    }

    // 버프 보정
    for buff in &self.active_buffs {
        base = buff.apply(base);
    }

    base
}
```

### 5.4 스탯 보정 캡

- 최소값: 1 (스탯이 0이 되지 않도록)
- 최대값: 255 (u32 범위 내, 임의 상한)
- 버프/디버프 적용 순서: 기본 스탯 → 장비 → 버프/디버프

---

## 6. 현재 구현 vs 미구현

### ✅ 구현 완료

| 기능 | 위치 | 상태 |
|------|------|------|
| 기본 캐릭터 생성 | `Character::new()` | ✅ 완료 |
| 클래스별 기본 스탯 | `CharacterClass::base_stats()` | ✅ 완료 |
| HP/MP 초기 계산 | 생성자 내 로직 | ✅ 완료 |
| 레벨업 (HP 성장) | `gain_experience()` | ✅ 완료 |
| 데미지 처리 | `take_damage()` | ✅ 완료 |
| 회복 처리 | `heal()` | ✅ 완료 |
| 생존 판정 | `is_alive()` | ✅ 완료 |
| 경험치 필요량 계산 | `xp_for_next_level()` | ✅ 완료 |

### ❌ 미구현

| 기능 | 우선순위 | 예상 작업량 | 비고 |
|------|----------|-------------|------|
| 클래스 고유 능력 | 🔴 높음 | Large | 스킬 시스템 전체 구현 필요 |
| 스탯 포인트 할당 | 🟡 중간 | Medium | 레벨업 시 분배 방식 |
| MP 소모/회복 시스템 | 🟡 중간 | Medium | MP 관련 모든 메커니즘 |
| 장비 시너지 | 🟢 낮음 | Large | 장비 시스템과 연동 |
| 최대 레벨 검증 | 🟢 낮음 | Small | 간단한 경계 검사 |
| 캐릭터 삭제/리셋 | 🟢 낮음 | Small | 영구 삭제 로직 |
| 사망 시 패널티 | 🟡 중간 | Medium | 경험치 속실/부활 메커니즘 |

---

## 7. 확장 고려사항

### 7.1 서브 클래스 시스템

레벨 30 도달 시 서브 클래스 선택 가능:
- Warrior → Berserker (공격 특화) / Guardian (방어 특화)
- Mage → Pyromancer (화염) / Frost Mage (빙결)
- Rogue → Assassin (도적) / Shadow Dancer (그림자)
- Cleric → Paladin (성기사) / Druid (드루이드)

### 7.2 레시피 시스템 연동

캐릭터 레벨에 따라 해금되는 제작 레시피:
- 레벨 5: 기본 무기 제작
- 레벨 15: 고급 무기 제작
- 레벨 30: 전설급 장비 제작

### 7.3 달성 시스템

특정 조건 달성 시 보너스 스탯/타이틀 부여:
- 모든 몬스터 10마리 처치: STR +1
- 첫 사망 경험: CON +2
- 100레벨 달성: 모든 스탯 +5
