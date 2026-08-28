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
