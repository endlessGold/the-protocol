//! Presentation Command Protocol.
//!
//! See `docs/11-presentation/01-presentation-command-protocol.md` for the
//! full design rationale. In short: a small, stable, generic vocabulary the
//! core uses to "remote control" a game engine (Godot first, maybe Unity
//! later). The vocabulary is deliberately engine-agnostic and gdext-free -
//! nothing in this crate depends on Godot, so it builds and unit-tests on
//! its own. A future `bindings/godot` crate only needs to implement
//! `PresentationSink`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use protocol_domain::DomainEvent;

/// The value types every property/effect parameter is expressed in. Kept
/// deliberately small (design doc §3) - the moment this list grows,
/// everything downstream (shader params, stats, UI labels) has to grow with
/// it, which is exactly the churn this protocol exists to avoid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
}

impl From<i64> for PropertyValue {
    fn from(v: i64) -> Self {
        PropertyValue::Int(v)
    }
}
impl From<u32> for PropertyValue {
    fn from(v: u32) -> Self {
        PropertyValue::Int(v as i64)
    }
}
impl From<u64> for PropertyValue {
    fn from(v: u64) -> Self {
        PropertyValue::Int(v as i64)
    }
}
impl From<f64> for PropertyValue {
    fn from(v: f64) -> Self {
        PropertyValue::Float(v)
    }
}
impl From<bool> for PropertyValue {
    fn from(v: bool) -> Self {
        PropertyValue::Bool(v)
    }
}
impl From<String> for PropertyValue {
    fn from(v: String) -> Self {
        PropertyValue::Text(v)
    }
}
impl From<&str> for PropertyValue {
    fn from(v: &str) -> Self {
        PropertyValue::Text(v.to_string())
    }
}

/// What kind of thing is being spawned. The engine decides how each kind
/// actually looks (model, sprite, particle system, ...) - the core only
/// ever names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Player,
    Npc,
}

/// A command the core sends to the engine. See design doc §1 for the full
/// rationale and the mapping to domain concepts.
///
/// Serializable so it can travel two ways: in-process (embedded engine
/// calls `PresentationSink::send` directly) or over the wire, wrapped in a
/// `protocol_protocol::Event` (see docs/11-presentation §7) for a
/// networked multiplayer client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PresentationCommand {
    SpawnEntity {
        entity_id: u64,
        kind: EntityKind,
        room_id: u32,
        display_name: String,
    },
    DespawnEntity {
        entity_id: u64,
    },
    EnterRoom {
        entity_id: u64,
        room_id: u32,
    },
    LeaveRoom {
        entity_id: u64,
        room_id: u32,
    },
    UpdateProperty {
        entity_id: u64,
        key: String,
        value: PropertyValue,
    },
    /// Also the channel for shader/VFX parameters - the core names an
    /// effect and hands over arbitrary key/value data (e.g.
    /// `params={"color": "#ff3333", "intensity": 0.8}`); it never needs to
    /// know these end up as shader uniforms. See design doc §1.
    PlayEffect {
        name: String,
        entity_id: Option<u64>,
        params: HashMap<String, PropertyValue>,
    },
    ShowMessage {
        text: String,
        target_entity_id: Option<u64>,
    },
}

/// A command the engine sends back to the core. See design doc §2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineInput {
    Action {
        action: String,
        payload: HashMap<String, PropertyValue>,
    },
    Tick {
        delta_seconds: f64,
    },
}

/// Anything that can receive `PresentationCommand`s. The (future) Godot
/// binder implements this by re-emitting each command as a Godot signal;
/// tests implement it by collecting commands into a `Vec` (see `VecSink`).
/// No engine-specific dependency belongs in this trait.
pub trait PresentationSink {
    fn send(&mut self, command: PresentationCommand);
}

/// A `PresentationSink` that just collects everything - useful for testing
/// the translator below (or anything upstream of it) without a real engine.
#[derive(Debug, Default)]
pub struct VecSink {
    pub sent: Vec<PresentationCommand>,
}

impl PresentationSink for VecSink {
    fn send(&mut self, command: PresentationCommand) {
        self.sent.push(command);
    }
}

/// Translate a `DomainEvent` into zero or more `PresentationCommand`s.
///
/// This is a *partial* mapping, deliberately - design doc §6 explains why.
/// `GameWorld::start_combat()` now calls `Combat::process_attack()` (fixed
/// 2026-08-28, the-protocol#26/#27), so `AttackExecuted`/`CombatEnded`/
/// combat-driven `LevelUp` events do fire on the live path. `CombatStarted`
/// still maps to nothing on purpose rather than a guessed-at command - see
/// the match arm.
pub fn translate_event(event: &DomainEvent) -> Vec<PresentationCommand> {
    match event {
        DomainEvent::CharacterCreated { character_id, name } => vec![PresentationCommand::SpawnEntity {
            entity_id: *character_id,
            kind: EntityKind::Player,
            // CharacterCreated doesn't carry a room_id - new characters
            // always start in room 1 today (application::GameWorld::
            // create_character). If that ever changes, this event needs a
            // room_id field too.
            room_id: 1,
            display_name: name.clone(),
        }],

        DomainEvent::LevelUp { character_id, new_level } => vec![
            PresentationCommand::UpdateProperty {
                entity_id: *character_id,
                key: "level".to_string(),
                value: (*new_level).into(),
            },
            PresentationCommand::PlayEffect {
                name: "level_up".to_string(),
                entity_id: Some(*character_id),
                params: HashMap::new(),
            },
        ],

        // Deliberately does NOT also emit UpdateProperty{key:"hp", ...} -
        // the event only carries the damage *delta*, not the resulting hp.
        // Computing the new value needs a GameWorld/Character lookup this
        // pure event->command translator doesn't have; whatever eventually
        // dispatches DomainEvents needs to look up current hp itself and
        // emit that UpdateProperty alongside this one.
        DomainEvent::AttackExecuted { attacker_id, target_id, damage, .. } => vec![PresentationCommand::PlayEffect {
            name: "hit".to_string(),
            entity_id: Some(*target_id),
            params: HashMap::from([
                ("damage".to_string(), (*damage).into()),
                ("attacker_id".to_string(), (*attacker_id).into()),
            ]),
        }],

        DomainEvent::CombatEnded { winner_id, loser_id, .. } => vec![
            PresentationCommand::DespawnEntity { entity_id: *loser_id },
            PresentationCommand::ShowMessage {
                text: format!("Combat ended - {} defeated {}", winner_id, loser_id),
                target_entity_id: None,
            },
        ],

        DomainEvent::PlayerEnteredRoom { player_id, room_id } => vec![PresentationCommand::EnterRoom {
            entity_id: *player_id,
            room_id: *room_id,
        }],

        DomainEvent::PlayerLeftRoom { player_id, room_id } => vec![PresentationCommand::LeaveRoom {
            entity_id: *player_id,
            room_id: *room_id,
        }],

        DomainEvent::ItemAcquired { player_id, item_id, quantity } => vec![PresentationCommand::ShowMessage {
            text: format!("Picked up {}x item #{}", quantity, item_id),
            target_entity_id: Some(*player_id),
        }],

        DomainEvent::ItemRemoved { player_id, item_id, quantity } => vec![PresentationCommand::ShowMessage {
            text: format!("Lost {}x item #{}", quantity, item_id),
            target_entity_id: Some(*player_id),
        }],

        // No player-visible presentation beyond what AttackExecuted's
        // PlayEffect already implies. Left unmapped rather than guessing.
        DomainEvent::CombatStarted { .. } => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_created_spawns_a_player_entity() {
        let event = DomainEvent::CharacterCreated {
            character_id: 7,
            name: "Aldric".to_string(),
        };
        let commands = translate_event(&event);
        assert_eq!(
            commands,
            vec![PresentationCommand::SpawnEntity {
                entity_id: 7,
                kind: EntityKind::Player,
                room_id: 1,
                display_name: "Aldric".to_string(),
            }]
        );
    }

    #[test]
    fn level_up_updates_property_and_plays_effect() {
        let event = DomainEvent::LevelUp { character_id: 7, new_level: 5 };
        let commands = translate_event(&event);
        assert_eq!(commands.len(), 2);
        assert!(matches!(&commands[0], PresentationCommand::UpdateProperty { key, .. } if key == "level"));
        assert!(matches!(&commands[1], PresentationCommand::PlayEffect { name, .. } if name == "level_up"));
    }

    #[test]
    fn attack_executed_carries_damage_as_an_effect_param() {
        let event = DomainEvent::AttackExecuted {
            combat_id: 1,
            attacker_id: 1,
            target_id: 2,
            damage: 42,
        };
        let commands = translate_event(&event);
        match &commands[0] {
            PresentationCommand::PlayEffect { name, entity_id, params } => {
                assert_eq!(name, "hit");
                assert_eq!(*entity_id, Some(2));
                assert_eq!(params.get("damage"), Some(&PropertyValue::Int(42)));
            }
            other => panic!("expected PlayEffect, got {:?}", other),
        }
    }

    #[test]
    fn combat_started_has_no_mapping_yet() {
        let event = DomainEvent::CombatStarted { combat_id: 1, attacker_id: 1, target_id: 2 };
        assert_eq!(translate_event(&event), vec![]);
    }

    #[test]
    fn vec_sink_collects_everything_sent() {
        let mut sink = VecSink::default();
        sink.send(PresentationCommand::DespawnEntity { entity_id: 1 });
        sink.send(PresentationCommand::ShowMessage {
            text: "bye".to_string(),
            target_entity_id: None,
        });
        assert_eq!(sink.sent.len(), 2);
    }
}
