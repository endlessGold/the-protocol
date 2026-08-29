use crate::event::DomainEvent;

/// The minimal shared surface `Character` and `Npc` both need to
/// participate in `Combat::process_attack`.
///
/// This exists because `Character` and `Npc` used to be incompatible
/// types — `Combat::process_attack(&mut Character, &mut Character)` could
/// only ever be called with two `Character`s, so
/// `application::GameWorld::start_combat()` (which fights a `Character`
/// against an `Npc`) had to fabricate a throwaway `Character` out of `Npc`
/// fields (with hardcoded `Stats{5,5,5,5,10}` for *every* NPC regardless of
/// how it was described) just to reuse the damage formula, and never called
/// `process_attack()` at all — so no `DomainEvent` ever came out of a real
/// fight. See docs/11-presentation/01-presentation-command-protocol.md §6.
pub trait Combatant {
    fn combatant_id(&self) -> u64;
    fn combatant_name(&self) -> &str;
    fn hp(&self) -> u32;
    fn max_hp(&self) -> u32;
    /// Used to size XP awards when this combatant is defeated
    /// (`100 * level`, matching the existing formula in
    /// `Combat::process_attack`).
    fn level(&self) -> u32;
    fn take_damage(&mut self, amount: u32) -> u32;
    fn is_alive(&self) -> bool {
        self.hp() > 0
    }
    /// Effective offensive power for damage calculation (`Character` uses
    /// `stats.strength`).
    fn offense(&self) -> u32;
    /// Effective defensive power for damage calculation (`Character` uses
    /// `stats.constitution`).
    fn defense(&self) -> u32;
    /// Award experience for winning a fight. Only `Character` actually
    /// levels up; the default no-op lets `Npc` (and anything else that can
    /// fight but never levels) implement `Combatant` without pretending to
    /// have a level-up system.
    fn grant_experience(&mut self, _xp: u64) -> Vec<DomainEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{Character, CharacterClass};
    use crate::world::Npc;

    fn npc(hp: u32, attack: u32, defense: u32) -> Npc {
        Npc {
            id: 1,
            name: "Test Dummy".to_string(),
            description: "For hitting.".to_string(),
            room_id: 1,
            hp,
            max_hp: hp,
            level: 2,
            attack,
            defense,
        }
    }

    /// Both impls must agree on the contract, since `Combat` only ever sees
    /// them through `&mut dyn Combatant` - a divergence here shows up as a
    /// confusing combat bug rather than a type error.
    #[test]
    fn both_impls_report_identity_and_health() {
        let character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let dummy = npc(40, 7, 3);

        for c in [&character as &dyn Combatant, &dummy as &dyn Combatant] {
            assert!(!c.combatant_name().is_empty());
            assert!(c.hp() > 0);
            assert!(c.max_hp() >= c.hp());
            assert!(c.is_alive());
            assert!(c.level() >= 1);
        }
        assert_eq!(dummy.combatant_id(), 1);
    }

    #[test]
    fn take_damage_saturates_at_zero_and_flips_is_alive() {
        let mut dummy = npc(10, 5, 1);
        assert_eq!(dummy.take_damage(4), 4);
        assert_eq!(dummy.hp(), 6);

        // Overkill must not underflow - hp is u32.
        assert_eq!(dummy.take_damage(999), 6, "should only absorb remaining hp");
        assert_eq!(dummy.hp(), 0);
        assert!(!dummy.is_alive());
    }

    #[test]
    fn character_offense_defense_come_from_stats() {
        let character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        assert_eq!(character.offense(), character.stats.strength);
        assert_eq!(character.defense(), character.stats.constitution);
    }

    #[test]
    fn npc_offense_defense_are_per_instance() {
        // Before Npc carried its own stats, every NPC shared a hardcoded
        // stat block, so a Goblin hit exactly as hard as a Town Guard.
        let weak = npc(10, 3, 1);
        let strong = npc(100, 20, 15);
        assert_ne!(weak.offense(), strong.offense());
        assert_ne!(weak.defense(), strong.defense());
    }

    #[test]
    fn only_characters_gain_experience() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let before = character.experience;
        let events = character.grant_experience(50);
        assert_eq!(character.experience, before + 50);
        assert!(events.is_empty(), "50 xp shouldn't level a fresh character");

        // NPCs use the default no-op: they never level, and must not panic.
        let mut dummy = npc(10, 5, 1);
        assert!(dummy.grant_experience(10_000).is_empty());
        assert_eq!(dummy.level(), 2, "npc level should be untouched");
    }

    #[test]
    fn enough_experience_levels_up_and_emits_an_event() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let start_level = character.level;
        let events = character.grant_experience(character.xp_for_next_level());

        assert!(character.level > start_level);
        assert!(matches!(
            events.first(),
            Some(crate::event::DomainEvent::LevelUp { .. })
        ));
    }
}
