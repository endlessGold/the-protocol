# 인벤토리 시스템 상세 설계

> 모듈: `domain::inventory`
> 소스: `domain/src/inventory.rs`

---

## 1. 인벤토리 구조

### 1.1 Inventory 엔티티

```rust
pub struct Inventory {
    pub items: Vec<ItemStack>,  // 아이템 스택 목록
    pub capacity: usize,       // 최대 슬롯 수
    pub gold: u64,             // 보유 골드
}
```

### 1.2 ItemStack 구조체

```rust
pub struct ItemStack {
    pub item_id: u32,    // 아이템 고유 ID
    pub name: String,    // 아이템 이름 (표시용)
    pub quantity: u32,   // 보유 수량
}
```

### 1.3 필드별 제약조건

| 필드 | 타입 | 제약조건 | 기본값 | 비고 |
|------|------|----------|--------|------|
| `items` | `Vec<ItemStack>` | `len ≤ capacity` | `vec![]` | 슬롯 기반 관리 |
| `capacity` | `usize` | `> 0` | `20` | 확장 가능 |
| `gold` | `u64` | `≥ 0` | `0` | 골드는 슬롯 미차감 |
| `item_id` | `u32` | 유니크 | - | Item 테이블 FK |
| `quantity` | `u32` | `≥ 1` | - | 0이면 스택 제거 |
| `name` | `String` | `1~64자` | - | 표시용 |

---

## 2. 아이템 스택 시스템

### 2.1 스택 추가 로직

```rust
pub fn add_item(&mut self, item_id: u32, name: &str, quantity: u32) -> Result<(), InventoryError> {
    // 1. 기존 스택에 추가 시도
    if let Some(stack) = self.items.iter_mut().find(|s| s.item_id == item_id) {
        stack.quantity += quantity;
        return Ok(());
    }

    // 2. 새 슬롯 필요 시 용량 검사
    if self.items.len() >= self.capacity {
        return Err(InventoryError::Full);
    }

    // 3. 새 스택 생성
    self.items.push(ItemStack {
        item_id,
        name: name.to_string(),
        quantity,
    });
    Ok(())
}
```

### 2.2 스택 제거 로직

```rust
pub fn remove_item(&mut self, item_id: u32, quantity: u32) -> Result<(), InventoryError> {
    if let Some(stack) = self.items.iter_mut().find(|s| s.item_id == item_id) {
        if stack.quantity < quantity {
            return Err(InventoryError::InsufficientQuantity);
        }
        stack.quantity -= quantity;
        if stack.quantity == 0 {
            self.items.retain(|s| s.item_id != item_id);  // 빈 스택 제거
        }
        Ok(())
    } else {
        Err(InventoryError::ItemNotFound(item_id))
    }
}
```

### 2.3 스택 규칙

| 규칙 | 설명 |
|------|------|
| 같은 ID는 하나의 스택 | `item_id`가 같으면 하나의 `ItemStack`에 합침 |
| 수량 무제한 | 같은 아이템은 슬롯 하나에 무제한 스택 가능 |
| 슬롯 점유 | 스택당 슬롯 1개 차지 |
| 빈 스택 자동 제거 | `quantity == 0`이면 `items`에서 제거 |
| 골드는 별도 관리 | `gold`는 스택에 포함되지 않음 |

---

## 3. 아이템 타입

### 3.1 ItemType 열거형

```rust
pub enum ItemType {
    Weapon,      // 무기
    Armor,       // 방어구
    Consumable,  // 소모품
    Quest,       // 퀘스트 아이템
    Material,    // 재료
}
```

### 3.2 아이템 타입별 특성

| 타입 | 스택 가능 | 사용 가능 | 판매 가능 | 비고 |
|------|-----------|-----------|-----------|------|
| Weapon | ❌ (1개만) | ❌ (장착만) | ✅ | 장비 슬롯에 장착 |
| Armor | ❌ (1개만) | ❌ (장착만) | ✅ | 장비 슬롯에 장착 |
| Consumable | ✅ | ✅ | ✅ | 사용 시 효과 적용 |
| Quest | ❌ (1개만) | ❌ | ❌ | 퀘스트 완료 시 자동 제거 |
| Material | ✅ | ❌ | ✅ | 제작에 사용 |

### 3.3 아이템 예시

| ID | 이름 | 타입 | 희귀도 | 효과 | 가치 |
|----|------|------|--------|------|------|
| 1 | 낡은 검 | Weapon | Common | STR +3 | 50g |
| 2 | 가죽 갑옷 | Armor | Common | CON +2 | 30g |
| 3 | 체력 포션 | Consumable | Common | HP +30 회복 | 10g |
| 4 | 마나 포션 | Consumable | Common | MP +20 회복 | 10g |
| 5 | 고블린 이빨 | Material | Common | - | 5g |

---

## 4. 희귀도 시스템 (설계)

### 4.1 희귀도 등급

```rust
pub enum Rarity {
    Common,     // 일반 - 회색
    Uncommon,   // 고급 - 녹색
    Rare,       // 희귀 - 파란색
    Epic,       // 영웅 - 보라색
    Legendary,  // 전설 - 주황색
}
```

### 4.2 희귀도별 배율

| 등급 | 드롭 배율 | 스탯 보너스 | 판매 배율 | 색상 코드 |
|------|-----------|-------------|-----------|-----------|
| Common | 100% | 기본 | ×1.0 | #FFFFFF (흰색) |
| Uncommon | 50% | +10% | ×2.0 | #1EFF00 (녹색) |
| Rare | 20% | +25% | ×5.0 | #0070DD (파란색) |
| Epic | 5% | +50% | ×15.0 | #A335EE (보라색) |
| Legendary | 1% | +100% | ×50.0 | #FF8000 (주황색) |

### 4.3 희귀도가 장비에 미치는 영향

```rust
// 장비 스탯 계산 시 희귀도 적용 (설계)
pub fn effective_stat(base_stat: u32, rarity: &Rarity) -> u32 {
    let multiplier = match rarity {
        Rarity::Common => 1.0,
        Rarity::Uncommon => 1.1,
        Rarity::Rare => 1.25,
        Rarity::Epic => 1.5,
        Rarity::Legendary => 2.0,
    };
    (base_stat as f64 * multiplier) as u32
}
```

---

## 5. 장비 시스템 (설계)

### 5.1 Equipment 슬롯

```rust
pub enum EquipmentSlot {
    Weapon,     // 무기
    Armor,      // 방어구
    Shield,     // 방패
    Accessory,  // 장신구 (반지, 목걸이 등)
}

pub struct Equipment {
    pub slots: HashMap<EquipmentSlot, Item>,
}
```

### 5.2 슬롯 구성

| 슬롯 | 허용 타입 | 스탯 영향 | 비고 |
|------|-----------|-----------|------|
| Weapon | Weapon | STR, DEX | 공격력 |
| Armor | Armor | CON | 방어력 |
| Shield | Armor (방패류) | CON, DEX | 방어력 + 회피 |
| Accessory | 장신구류 | ALL | 다양 |

### 5.3 장착/해제 로직

```rust
impl Character {
    pub fn equip(&mut self, item: Item) -> Result<Item, EquipError> {
        let slot = match item.item_type {
            ItemType::Weapon => EquipmentSlot::Weapon,
            ItemType::Armor => EquipmentSlot::Armor,
            _ => return Err(EquipError::InvalidItemType),
        };

        // 기존 장비 해제
        let old_item = self.equipment.remove(&slot);

        // 새 장비 장착
        self.equipment.insert(slot, item.clone());

        // 인벤토리에서 제거
        self.inventory.remove_item(item.id, 1)?;

        Ok(old_item.unwrap_or_default())
    }

    pub fn unequip(&mut self, slot: EquipmentSlot) -> Result<Item, EquipError> {
        let item = self.equipment.remove(&slot)
            .ok_or(EquipError::SlotEmpty)?;

        // 인벤토리에 추가
        self.inventory.add_item(item.id, &item.name, 1)?;

        Ok(item)
    }
}
```

### 5.4 장비 스탯 보너스

```rust
pub fn effective_stats(&self) -> Stats {
    let mut stats = self.stats.clone();

    for (_, item) in &self.equipment {
        stats.strength += item.bonus_strength;
        stats.dexterity += item.bonus_dexterity;
        stats.intelligence += item.bonus_intelligence;
        stats.wisdom += item.bonus_wisdom;
        stats.constitution += item.bonus_constitution;
    }

    stats
}
```

### 5.5 장비 시너지 (설계)

특정 장비 조합 시 추가 보너스:

| 조합 | 조건 | 보너스 |
|------|------|--------|
| 전사 세트 | Weapon + Armor (같은 세트) | CON +5 |
| 도적 세트 | Weapon + Accessory (같은 세트) | DEX +5 |
| 마법 세트 | Weapon + Shield (같은 세트) | INT +5 |

---

## 6. 아이템 사용 (Consumable)

### 6.1 사용 가능 아이템

| 아이템 | 효과 | MP 소모 | 쿨다운 |
|--------|------|---------|--------|
| 체력 포션 | HP +30 회복 | 0 | 3초 |
| 마나 포션 | MP +20 회복 | 0 | 3초 |
| 해독약 | 독 해제 | 0 | 0 |
| 텔레 포트 스크롤 | 시작 방으로 이동 | 0 | 0 |

### 6.2 사용 로직

```rust
pub fn use_item(&mut self, item_id: u32) -> Result<ItemEffect, UseItemError> {
    let item = self.inventory.get_item(item_id)
        .ok_or(UseItemError::ItemNotFound)?;

    match item.item_type {
        ItemType::Consumable => {
            let effect = match item.effect {
                ItemEffect::HealHp(amount) => {
                    self.heal(amount);
                    ItemEffect::HealHp(amount)
                }
                ItemEffect::HealMp(amount) => {
                    self.mp = (self.mp + amount).min(self.max_mp);
                    ItemEffect::HealMp(amount)
                }
                _ => return Err(UseItemError::CannotUse),
            };

            self.inventory.remove_item(item_id, 1)?;
            Ok(effect)
        }
        _ => Err(UseItemError::CannotUse),
    }
}
```

---

## 7. 아이템 드롭/교환

### 7.1 아이템 드롭

```rust
pub fn drop_item(&mut self, item_id: u32, quantity: u32) -> Result<ItemStack, InventoryError> {
    self.remove_item(item_id, quantity)?;

    Ok(ItemStack {
        item_id,
        name: self.get_item_name(item_id),
        quantity,
    })
}
```

### 7.2 아이템 교환 (설계)

```rust
pub fn trade(
    player_a: &mut Character,
    player_b: &mut Character,
    item_a: (u32, u32),  // (item_id, quantity)
    item_b: (u32, u32),  // (item_id, quantity)
) -> Result<(), TradeError> {
    // 1. 각 플레이어 보유 아이템 확인
    // 2. 아이템 교환 실행
    // 3. 인벤토리 용량 검사

    Ok(())
}
```

---

## 8. 인벤토리 용량 확장

### 8.1 확장 방법

| 방법 | 비용 | 확장량 | 비고 |
|------|------|--------|------|
| 가방 구매 | 100g × 현재 용량 | +5 슬롯 | 상점에서 구매 |
| 퀘스트 보상 | 무료 | +10 슬롯 | 특정 퀘스트 완료 |
| 업그레이드 | 재료 필요 | +5 슬롯 | 제작 시스템 |

### 8.2 확장 로직

```rust
pub fn expand_capacity(&mut self, additional: usize) -> Result<(), InventoryError> {
    let new_capacity = self.capacity + additional;
    if new_capacity > MAX_CAPACITY {
        return Err(InventoryError::CapacityExceeded);
    }
    self.capacity = new_capacity;
    Ok(())
}
```

### 8.3 최대 용량

- **기본**: 20 슬롯
- **최대**: 100 슬롯
- **증가 단위**: 5 슬롯씩

---

## 9. 전리품 시스템 (NPC 사망 시)

### 9.1 전리품 테이블

```rust
pub struct LootTable {
    pub entries: Vec<LootEntry>,
}

pub struct LootEntry {
    pub item_id: u32,
    pub drop_rate: f64,      // 0.0 ~ 1.0
    pub min_quantity: u32,
    pub max_quantity: u32,
}
```

### 9.2 드롭 계산

```rust
pub fn calculate_loot(loot_table: &LootTable) -> Vec<ItemStack> {
    let mut drops = Vec::new();
    let mut rng = rand::thread_rng();

    for entry in &loot_table.entries {
        let roll: f64 = rng.gen();
        if roll <= entry.drop_rate {
            let quantity = rng.gen_range(entry.min_quantity..=entry.max_quantity);
            drops.push(ItemStack {
                item_id: entry.item_id,
                name: get_item_name(entry.item_id),
                quantity,
            });
        }
    }

    drops
}
```

### 9.3 NPC별 전리품 예시

| NPC | 아이템 | 드롭률 | 수량 |
|-----|--------|--------|------|
| 고블린 | 고블린 이빨 | 80% | 1~3 |
| 고블린 | 낡은 단도 | 20% | 1 |
| 늑대 | 늑대 가죽 | 70% | 1~2 |
| 늑대 | 날카로운 이빨 | 30% | 1 |
| 트롤 | 트롤 피부 | 50% | 1 |
| 트롤 | 거대한 뼈 | 10% | 1 |

---

## 10. 현재 구현 vs 미구현

### ✅ 구현 완료

| 기능 | 위치 | 상태 |
|------|------|------|
| Inventory 구조체 | `Inventory` | ✅ 완료 |
| 아이템 스택 추가 | `add_item()` | ✅ 완료 |
| 아이템 스택 제거 | `remove_item()` | ✅ 완료 |
| 아이템 보유 확인 | `has_item()` | ✅ 완료 |
| 아이템 수량 조회 | `item_count()` | ✅ 완료 |
| 용량 검사 | `add_item()` 내 | ✅ 완료 |
| 에러 처리 | `InventoryError` | ✅ 완료 |
| 기본 용량 20 | `Inventory::new()` | ✅ 완료 |
| 골드 보유 | `gold` 필드 | ✅ 완료 |

### ❌ 미구현

| 기능 | 우선순위 | 예상 작업량 |
|------|----------|-------------|
| 장비 시스템 (장착/해제) | 🔴 높음 | Large |
| 아이템 사용 (Consumable) | 🔴 높음 | Medium |
| 희귀도 시스템 | 🟡 중간 | Medium |
| 전리품 시스템 | 🟡 중간 | Medium |
| 인벤토리 용량 확장 | 🟢 낮음 | Small |
| 아이템 교환 | 🟢 낮음 | Medium |
| 장비 시너지 | 🟢 낮음 | Medium |
| 아이템 설명/도감 | 🟢 낮음 | Small |
| 골드 사용 (상점) | 🟡 중간 | Medium |

---

## 11. 확장 고려사항

### 11.1 제작 시스템

재료를 조합하여 아이템 제작:

```rust
pub struct CraftingRecipe {
    pub result_item_id: u32,
    pub result_quantity: u32,
    pub required_items: Vec<(u32, u32)>,  // (item_id, quantity)
    pub required_level: u32,
}
```

### 11.2 아이템 강화

기존 아이템에 추가 스탯 부여:

```
강화 레벨 1: 기본 스탯 +10%
강화 레벨 2: 기본 스탯 +20%
강화 레벨 3: 기본 스탯 +35%
강화 레벨 4: 기본 스탯 +50%
강화 레벨 5: 기본 스탯 +75% (최대)
```

### 11.3 인벤토리 정렬/필터

```rust
pub enum SortMode {
    ByName,
    ByType,
    ByRarity,
    ByQuantity,
    ByValue,
}

pub enum FilterMode {
    All,
    ByType(ItemType),
    ByRarity(Rarity),
}
```
