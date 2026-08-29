use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("Inventory full")]
    Full,

    #[error("Item not found: {0}")]
    ItemNotFound(u32),

    #[error("Insufficient quantity")]
    InsufficientQuantity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub items: Vec<ItemStack>,
    pub capacity: usize,
    pub gold: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_id: u32,
    pub name: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub item_type: ItemType,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemType {
    Weapon,
    Armor,
    Consumable,
    Quest,
    Material,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            capacity: 20,
            gold: 0,
        }
    }

    pub fn add_item(
        &mut self,
        item_id: u32,
        name: &str,
        quantity: u32,
    ) -> Result<(), InventoryError> {
        if let Some(stack) = self.items.iter_mut().find(|s| s.item_id == item_id) {
            stack.quantity += quantity;
            return Ok(());
        }

        if self.items.len() >= self.capacity {
            return Err(InventoryError::Full);
        }

        self.items.push(ItemStack {
            item_id,
            name: name.to_string(),
            quantity,
        });
        Ok(())
    }

    pub fn remove_item(&mut self, item_id: u32, quantity: u32) -> Result<(), InventoryError> {
        if let Some(stack) = self.items.iter_mut().find(|s| s.item_id == item_id) {
            if stack.quantity < quantity {
                return Err(InventoryError::InsufficientQuantity);
            }
            stack.quantity -= quantity;
            if stack.quantity == 0 {
                self.items.retain(|s| s.item_id != item_id);
            }
            Ok(())
        } else {
            Err(InventoryError::ItemNotFound(item_id))
        }
    }

    pub fn has_item(&self, item_id: u32, quantity: u32) -> bool {
        self.items
            .iter()
            .find(|s| s.item_id == item_id)
            .map(|s| s.quantity >= quantity)
            .unwrap_or(false)
    }

    pub fn item_count(&self, item_id: u32) -> u32 {
        self.items
            .iter()
            .find(|s| s.item_id == item_id)
            .map(|s| s.quantity)
            .unwrap_or(0)
    }
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}
