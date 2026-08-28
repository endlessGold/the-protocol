use serde::{Deserialize, Serialize};

use crate::combatant::Combatant;
use crate::event::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combat {
    pub id: u64,
    pub attacker_id: u64,
    pub target_id: u64,
    pub state: CombatState,
    pub turn: u32,
    pub log: Vec<CombatAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CombatState {
    InProgress,
    Finished { winner_id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatAction {
    pub actor_id: u64,
    pub action_type: CombatActionType,
    pub damage: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CombatActionType {
    Attack,
    Defend,
}

impl Combat {
    pub fn new(attacker_id: u64, target_id: u64) -> Self {
        Self {
            id: 0,
            attacker_id,
            target_id,
            state: CombatState::InProgress,
            turn: 1,
            log: Vec::new(),
        }
    }

    pub fn calculate_damage(attacker: &dyn Combatant, target: &dyn Combatant) -> u32 {
        use rand::Rng;

        let base_damage = attacker.offense() as f64;
        let defense = target.defense() as f64 * 0.5;
        let raw_damage = (base_damage - defense).max(1.0);

        let mut rng = rand::thread_rng();
        let variance = raw_damage * 0.2;
        let final_damage = raw_damage + rng.gen_range(-variance..variance);
        final_damage.max(1.0) as u32
    }

    /// Resolve one attack. `attacker`/`target` can be any mix of `Character`
    /// and `Npc` (both implement `Combatant`) - this is what
    /// `application::GameWorld::start_combat()` should call instead of
    /// hand-rolling damage math against a fabricated fake `Character`.
    pub fn process_attack(
        &mut self,
        attacker: &mut dyn Combatant,
        target: &mut dyn Combatant,
    ) -> Vec<DomainEvent> {
        let mut events = Vec::new();

        let damage = Self::calculate_damage(&*attacker, &*target);
        target.take_damage(damage);

        self.log.push(CombatAction {
            actor_id: self.attacker_id,
            action_type: CombatActionType::Attack,
            damage: Some(damage),
            message: format!(
                "{} hits {} for {} damage!",
                attacker.combatant_name(),
                target.combatant_name(),
                damage
            ),
        });

        events.push(DomainEvent::AttackExecuted {
            combat_id: self.id,
            attacker_id: self.attacker_id,
            target_id: self.target_id,
            damage,
        });

        if !target.is_alive() {
            self.state = CombatState::Finished {
                winner_id: self.attacker_id,
            };
            let xp = 100 * target.level() as u64;
            let level_events = attacker.grant_experience(xp);
            events.extend(level_events);

            events.push(DomainEvent::CombatEnded {
                combat_id: self.id,
                winner_id: self.attacker_id,
                loser_id: self.target_id,
            });
        }

        self.turn += 1;
        events
    }
}
