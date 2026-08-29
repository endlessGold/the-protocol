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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{Character, CharacterClass};
    use crate::world::Npc;

    fn hero() -> Character {
        let mut c = Character::new("Hero".to_string(), CharacterClass::Warrior);
        c.id = 1000;
        c
    }

    fn dummy(hp: u32, defense: u32) -> Npc {
        Npc {
            id: 2,
            name: "Dummy".to_string(),
            description: String::new(),
            room_id: 1,
            hp,
            max_hp: hp,
            level: 1,
            attack: 5,
            defense,
        }
    }

    #[test]
    fn damage_is_never_zero_even_against_heavy_armour() {
        // raw = offense - defense*0.5, then +/-20% variance. A defender
        // whose defense exceeds the attacker's offense must still take
        // chip damage rather than 0 (or, worse, underflow).
        let attacker = dummy(50, 0); // attack 5
        let tank = dummy(50, 10_000);
        for _ in 0..50 {
            assert!(Combat::calculate_damage(&attacker, &tank) >= 1);
        }
    }

    #[test]
    fn a_survivable_hit_emits_only_attack_executed() {
        let mut attacker = hero();
        let mut target = dummy(10_000, 0); // can't die in one hit
        let mut combat = Combat::new(attacker.id, target.id);
        combat.id = 7;

        let events = combat.process_attack(&mut attacker, &mut target);

        assert_eq!(
            events.len(),
            1,
            "expected only AttackExecuted: {:?}",
            events
        );
        match &events[0] {
            DomainEvent::AttackExecuted {
                combat_id, damage, ..
            } => {
                assert_eq!(*combat_id, 7);
                assert!(*damage >= 1);
            }
            other => panic!("expected AttackExecuted, got {:?}", other),
        }
        assert!(target.hp < target.max_hp, "target should have taken damage");
        assert_eq!(combat.state, CombatState::InProgress);
        assert_eq!(combat.log.len(), 1);
    }

    #[test]
    fn a_killing_blow_ends_combat_and_awards_experience() {
        let mut attacker = hero();
        let mut target = dummy(1, 0); // dies to anything
        let xp_before = attacker.experience;
        let mut combat = Combat::new(attacker.id, target.id);

        let events = combat.process_attack(&mut attacker, &mut target);

        assert!(!target.is_alive());
        assert!(
            matches!(combat.state, CombatState::Finished { winner_id } if winner_id == attacker.id)
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::CombatEnded { .. })),
            "expected CombatEnded in {:?}",
            events
        );
        // AttackExecuted must come before CombatEnded - a client replaying
        // these in order should see the hit land, then the fight end.
        assert!(matches!(events[0], DomainEvent::AttackExecuted { .. }));
        assert!(matches!(
            events.last(),
            Some(DomainEvent::CombatEnded { .. })
        ));
        assert!(
            attacker.experience > xp_before || attacker.level > 1,
            "killing should award xp (or level up, consuming it)"
        );
    }

    #[test]
    fn turn_counter_advances_per_attack() {
        let mut attacker = hero();
        let mut target = dummy(10_000, 0);
        let mut combat = Combat::new(attacker.id, target.id);
        let start = combat.turn;

        combat.process_attack(&mut attacker, &mut target);
        combat.process_attack(&mut attacker, &mut target);

        assert_eq!(combat.turn, start + 2);
        assert_eq!(combat.log.len(), 2);
    }

    #[test]
    fn an_npc_can_attack_a_character() {
        // The whole point of Combatant: this direction has to work too, not
        // just character -> npc.
        let mut attacker = dummy(50, 0);
        let mut target = hero();
        let hp_before = target.hp;
        let mut combat = Combat::new(attacker.id, target.id);

        let events = combat.process_attack(&mut attacker, &mut target);

        assert!(target.hp < hp_before);
        assert!(matches!(events[0], DomainEvent::AttackExecuted { .. }));
    }
}
