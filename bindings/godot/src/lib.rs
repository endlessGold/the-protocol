//! Godot 4 GDExtension binding for The Protocol's core.
//!
//! This is the FFI bridge for the *embedded* integration path: Godot loads
//! this crate as a native library and drives an in-process `GameWorld`
//! directly, with no network involved. (The other path - a networked client
//! speaking the TCP protocol - needs none of this; see
//! `docs/11-presentation/01-presentation-command-protocol.md` §7.)
//!
//! Design: Godot never sees domain types. Every `#[func]` here returns a
//! plain `Dictionary`, and every state change is announced through ONE
//! generic signal carrying a command name + data, matching the
//! `PresentationCommand` vocabulary. That keeps this file nearly static as
//! the core evolves - see the design doc's rationale.
//!
//! ## Verification status
//!
//! The gdext API here was researched from the godot-rust book, the gdext
//! repo README, and docs.rs, then compiled for real by CI (the sandbox
//! this was authored in cannot run cargo at all - Windows Application
//! Control Policy, os error 4551 - so CI is the only compiler in the loop;
//! see .github/workflows/ci.yml's advisory `godot-bindings` job).
//!
//! Confirmed by an actual build: `godot = "0.5.5"` resolves, and the
//! `#[gdextension]`/`ExtensionLibrary`, `#[derive(GodotClass)]`,
//! `#[class(init, base=Node)]`, `#[init(val = ...)]`, `#[godot_api]`,
//! `#[func]` and `#[signal]` forms below all expand. One thing the
//! research got wrong and the compiler caught: gdext's `dict!` macro takes
//! `"key" => value`, not `"key": value`.
//!
//! This crate now compiles cleanly under CI. What remains unverified is
//! runtime behavior: no Godot binary was available to actually load the
//! library, instantiate ProtocolCore, or observe a signal reach GDScript.

use std::cell::RefCell;

use godot::prelude::*;

use protocol_application::GameWorld;
use protocol_domain::Direction;
use protocol_presentation::{translate_event, PresentationCommand, PropertyValue};

/// GDExtension entry point. HIGH confidence: this exact pattern (marker
/// struct + `#[gdextension] unsafe impl ExtensionLibrary`) appears verbatim
/// and identically in two independently-fetched book sources. The macro
/// generates the real `extern "C"` symbol - default name `gdext_rust_init`,
/// which is what `godot-client/bin/core.gdextension`'s `entry_symbol` must
/// match.
struct ProtocolExtension;

#[gdextension]
unsafe impl ExtensionLibrary for ProtocolExtension {}

/// The Godot-facing core object. Instantiate from GDScript as
/// `ProtocolCore.new()`.
///
/// `#[class(init, base=Node)]` uses gdext's generated constructor with
/// field-level defaults (MEDIUM-HIGH confidence: the `init` shorthand is
/// shown in the gdext README and the book's signals page; the alternative
/// is a manual `fn init(base: Base<Node>) -> Self` in an `#[godot_api] impl
/// INode` block, which the book's hello-world uses - switch to that if this
/// form is rejected).
///
/// `RefCell` rather than a lock: Godot calls into extension code from its
/// main thread, and nothing here spawns threads or holds a borrow across a
/// call back into Godot.
#[derive(GodotClass)]
#[class(init, base=Node)]
pub struct ProtocolCore {
    #[init(val = RefCell::new(GameWorld::new()))]
    world: RefCell<GameWorld>,

    /// The character this Godot client is controlling. Set by
    /// `create_character`; every other call needs it, mirroring how a
    /// networked session binds a player id after `create_character`.
    #[init(val = None)]
    character_id: Option<u64>,

    /// Required for any struct that declares signals (book: "required when
    /// declaring signals").
    base: Base<Node>,
}

#[godot_api]
impl ProtocolCore {
    /// Emitted once per `PresentationCommand` produced by a call.
    ///
    /// One generic signal instead of seven typed ones is deliberate: it
    /// maps 1:1 onto `PresentationBridge.apply(command_type, data)` in the
    /// godot-client project, keeps this boundary stable as commands are
    /// added, and avoids the less-verified typed-signal-with-arguments
    /// syntax. MEDIUM-HIGH confidence on `#[signal]` + `self.signals()`
    /// emission: the no-argument form is corroborated identically by two
    /// book sources; the with-arguments form comes from one.
    #[signal]
    fn presentation_command(command_type: GString, data: VarDictionary);

    /// Create the character this client controls, and bind it.
    /// Returns `{success: bool, character_id: int, error: String}`.
    #[func]
    fn create_character(&mut self, name: GString, class: GString) -> VarDictionary {
        let result = {
            let mut world = self.world.borrow_mut();
            world.create_character(name.to_string(), &class.to_string())
        };

        match result {
            Ok(character) => {
                let character_id = character.id;
                {
                    let mut world = self.world.borrow_mut();
                    world.add_character(character);
                }
                self.character_id = Some(character_id);
                self.flush_events();
                dict! {
                    "success" => true,
                    "character_id" => character_id as i64,
                    "error" => "",
                }
            }
            Err(e) => err_dict(&e.to_string()),
        }
    }

    /// Move the bound character. `direction` is one of
    /// north/south/east/west/up/down (also accepts n/s/e/w/u/d, per
    /// `Direction::from_str`).
    #[func]
    fn move_player(&mut self, direction: GString) -> VarDictionary {
        let Some(character_id) = self.character_id else {
            return err_dict(NO_CHARACTER);
        };
        let Some(dir) = Direction::from_str(&direction.to_string()) else {
            return err_dict(&format!("Unknown direction: {}", direction));
        };

        let result = {
            let mut world = self.world.borrow_mut();
            world.move_character(character_id, dir)
        };

        match result {
            Ok(move_result) => {
                self.flush_events();
                dict! {
                    "success" => true,
                    "room_name" => move_result.room_name,
                    "room_description" => move_result.room_description,
                    "error" => "",
                }
            }
            Err(e) => err_dict(&e.to_string()),
        }
    }

    /// Attack an NPC in the current room by (partial) name.
    #[func]
    fn attack(&mut self, target_name: GString) -> VarDictionary {
        let Some(character_id) = self.character_id else {
            return err_dict(NO_CHARACTER);
        };

        let result = {
            let mut world = self.world.borrow_mut();
            world.start_combat(character_id, &target_name.to_string())
        };

        match result {
            Ok(info) => {
                self.flush_events();
                dict! {
                    "success" => true,
                    "message" => info.message,
                    "damage" => info.damage as i64,
                    "target_name" => info.target_name,
                    "target_hp" => info.target_hp as i64,
                    "target_max_hp" => info.target_max_hp as i64,
                    "error" => "",
                }
            }
            Err(e) => err_dict(&e.to_string()),
        }
    }

    /// Describe the room the bound character is in.
    #[func]
    fn look(&mut self) -> VarDictionary {
        let Some(character_id) = self.character_id else {
            return err_dict(NO_CHARACTER);
        };

        let world = self.world.borrow();
        let Some(room_id) = world.get_character(character_id).map(|c| c.room_id) else {
            return err_dict("Character not found");
        };
        let Some(room) = world.look_room(room_id) else {
            return err_dict("Room not found");
        };

        let mut exits = VarArray::new();
        for exit in &room.exits {
            exits.push(&exit.to_variant());
        }

        let mut npcs = VarArray::new();
        for npc in &room.npcs {
            // Bound to a typed local first: `dict!` is generic over
            // Dictionary<K, V> and `.to_variant()` accepts any of them, so
            // inline `dict!{..}.to_variant()` leaves K/V unconstrained
            // (E0283). Same below and in get_inventory.
            let entry: VarDictionary = dict! {
                "id" => npc.id as i64,
                "name" => npc.name.clone(),
                "hp" => npc.hp as i64,
                "max_hp" => npc.max_hp as i64,
            };
            npcs.push(&entry.to_variant());
        }

        let mut players = VarArray::new();
        for player in &room.players {
            let entry: VarDictionary = dict! {
                "id" => player.id as i64,
                "name" => player.name.clone(),
                "level" => player.level as i64,
            };
            players.push(&entry.to_variant());
        }

        dict! {
            "success" => true,
            "room_name" => room.name,
            "room_description" => room.description,
            "exits" => &exits,
            "npcs" => &npcs,
            "players" => &players,
            "error" => "",
        }
    }

    /// Inventory contents of the bound character.
    #[func]
    fn get_inventory(&mut self) -> VarDictionary {
        let Some(character_id) = self.character_id else {
            return err_dict(NO_CHARACTER);
        };

        let world = self.world.borrow();
        let Some(inventory) = world.get_inventory(character_id) else {
            return err_dict("Character not found");
        };

        let mut items = VarArray::new();
        for item in &inventory.items {
            let entry: VarDictionary = dict! {
                "item_id" => item.item_id as i64,
                "name" => item.name.clone(),
                "quantity" => item.quantity as i64,
            };
            items.push(&entry.to_variant());
        }

        dict! {
            "success" => true,
            "items" => &items,
            "gold" => inventory.gold as i64,
            "error" => "",
        }
    }

    /// Spawn an NPC at runtime in `room_id`.
    /// Returns `{success, npc_id, error}`.
    #[func]
    fn spawn_npc(
        &mut self,
        npc_name: GString,
        description: GString,
        room_id: i64,
        level: i64,
        hp: i64,
        attack: i64,
        defense: i64,
    ) -> VarDictionary {
        let result = {
            let mut world = self.world.borrow_mut();
            world.spawn_npc(
                npc_name.to_string(),
                description.to_string(),
                room_id as u32,
                level.max(1) as u32,
                hp.max(1) as u32,
                attack.max(0) as u32,
                defense.max(0) as u32,
            )
        };

        match result {
            Ok(npc_id) => {
                self.flush_events();
                dict! {
                    "success" => true,
                    "npc_id" => npc_id as i64,
                    "error" => "",
                }
            }
            Err(e) => err_dict(&e.to_string()),
        }
    }

    /// Move an NPC one room in `direction`.
    /// Returns `{success, room_id, error}`.
    #[func]
    fn move_npc(&mut self, npc_id: i64, direction: GString) -> VarDictionary {
        let Some(dir) = Direction::from_str(&direction.to_string()) else {
            return err_dict(&format!("Unknown direction: {}", direction));
        };

        let result = {
            let mut world = self.world.borrow_mut();
            world.move_npc(npc_id as u64, dir)
        };

        match result {
            Ok(room_id) => {
                self.flush_events();
                dict! {
                    "success" => true,
                    "room_id" => room_id as i64,
                    "error" => "",
                }
            }
            Err(e) => err_dict(&e.to_string()),
        }
    }

    /// Remove an NPC from the world.
    #[func]
    fn despawn_npc(&mut self, npc_id: i64) -> VarDictionary {
        let result = {
            let mut world = self.world.borrow_mut();
            world.despawn_npc(npc_id as u64)
        };

        match result {
            Ok(()) => {
                self.flush_events();
                dict! { "success" => true, "error" => "" }
            }
            Err(e) => err_dict(&e.to_string()),
        }
    }

    /// Directions an NPC can currently move, as lowercase strings.
    #[func]
    fn npc_exits(&self, npc_id: i64) -> VarArray {
        let world = self.world.borrow();
        let mut out = VarArray::new();
        for exit in world.npc_exits(npc_id as u64) {
            out.push(&exit.to_variant());
        }
        out
    }

    /// The character id this client is bound to, or -1 if
    /// `create_character` hasn't been called yet.
    #[func]
    fn current_character_id(&self) -> i64 {
        self.character_id.map(|id| id as i64).unwrap_or(-1)
    }
}

impl ProtocolCore {
    /// Drain whatever `DomainEvent`s the last call produced, translate them
    /// to `PresentationCommand`s, and emit one signal each.
    ///
    /// Deliberately NOT a `#[func]` - it's internal plumbing every mutating
    /// method calls, not something GDScript should invoke.
    fn flush_events(&mut self) {
        let commands: Vec<PresentationCommand> = {
            let mut world = self.world.borrow_mut();
            world
                .drain_events()
                .iter()
                .flat_map(translate_event)
                .collect()
        };

        for command in commands {
            let (command_type, data) = command_to_dict(&command);
            self.signals()
                .presentation_command()
                .emit(&GString::from(command_type), &data);
        }
    }
}

const NO_CHARACTER: &str = "No character yet - call create_character first";

fn err_dict(message: &str) -> VarDictionary {
    dict! {
        "success" => false,
        "error" => message.to_string(),
    }
}

fn property_to_variant(value: &PropertyValue) -> Variant {
    match value {
        PropertyValue::Int(v) => v.to_variant(),
        PropertyValue::Float(v) => v.to_variant(),
        PropertyValue::Text(v) => v.to_variant(),
        PropertyValue::Bool(v) => v.to_variant(),
    }
}

/// Flatten a `PresentationCommand` into the `(type, data)` pair that
/// `PresentationBridge.apply()` on the GDScript side expects. Keys here
/// must stay in sync with `godot-client/autoload/presentation_bridge.gd`.
fn command_to_dict(command: &PresentationCommand) -> (&'static str, VarDictionary) {
    match command {
        PresentationCommand::SpawnEntity {
            entity_id,
            kind,
            room_id,
            display_name,
        } => (
            "SpawnEntity",
            dict! {
                "entity_id" => *entity_id as i64,
                "kind" => format!("{:?}", kind),
                "room_id" => *room_id as i64,
                "display_name" => display_name.clone(),
            },
        ),
        PresentationCommand::DespawnEntity { entity_id } => (
            "DespawnEntity",
            dict! { "entity_id" => *entity_id as i64 },
        ),
        PresentationCommand::EnterRoom { entity_id, room_id } => (
            "EnterRoom",
            dict! {
                "entity_id" => *entity_id as i64,
                "room_id" => *room_id as i64,
            },
        ),
        PresentationCommand::LeaveRoom { entity_id, room_id } => (
            "LeaveRoom",
            dict! {
                "entity_id" => *entity_id as i64,
                "room_id" => *room_id as i64,
            },
        ),
        PresentationCommand::UpdateProperty {
            entity_id,
            key,
            value,
        } => (
            "UpdateProperty",
            dict! {
                "entity_id" => *entity_id as i64,
                "key" => key.clone(),
                "value" => &property_to_variant(value),
            },
        ),
        PresentationCommand::PlayEffect {
            name,
            entity_id,
            params,
        } => {
            let mut param_dict = VarDictionary::new();
            for (k, v) in params {
                param_dict.set(k.clone(), &property_to_variant(v));
            }
            (
                "PlayEffect",
                dict! {
                    "name" => name.clone(),
                    // Godot has no null-int; -1 means "no entity", matching
                    // current_character_id()'s convention above.
                    "entity_id" => entity_id.map(|id| id as i64).unwrap_or(-1),
                    "params" => &param_dict,
                },
            )
        }
        PresentationCommand::ShowMessage {
            text,
            target_entity_id,
        } => (
            "ShowMessage",
            dict! {
                "text" => text.clone(),
                "target_entity_id" => target_entity_id.map(|id| id as i64).unwrap_or(-1),
            },
        ),
    }
}
