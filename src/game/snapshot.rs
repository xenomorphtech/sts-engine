use super::*;
use crate::creature::{Monster, Power};
use crate::map::{MapEdge, MapNode};
use serde_json::{json, Value};
use std::fmt::Debug;

fn snake_debug(value: impl Debug) -> String {
    let input = format!("{value:?}");
    let mut output = String::with_capacity(input.len() + 4);
    let mut previous_is_lower_or_digit = false;
    for ch in input.chars() {
        if ch.is_ascii_uppercase() {
            if previous_is_lower_or_digit {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_is_lower_or_digit = false;
        } else {
            output.push(ch.to_ascii_lowercase());
            previous_is_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    output
}

fn card(card: &Card) -> Value {
    json!({
        "id": card.sts_id(),
        "upgraded": card.upgraded,
        "times_upgraded": card.times_upgraded,
        "cost": card.cost,
        "cost_for_turn": card.cost_for_turn,
        "base_damage": card.base_damage,
        "base_block": card.base_block,
        "base_magic": card.base_magic,
        "misc": card.misc,
        "free_to_play_once": card.free_to_play_once,
        "exhaust": card.exhaust,
        "ethereal": card.ethereal,
        "retain": card.retain,
        "innate": card.innate,
        "in_bottle": card.in_bottle,
        "type": snake_debug(card.card_type()),
        "rarity": snake_debug(card.rarity()),
        "target": snake_debug(card.target()),
    })
}

fn cards(values: &[Card]) -> Vec<Value> {
    values.iter().map(card).collect()
}

fn power(power: &Power) -> Value {
    json!({
        "id": snake_debug(power.id),
        "amount": power.amount,
        "just_applied": power.just_applied,
        "skip_first": power.skip_first,
        "misc": power.misc,
    })
}

fn monster(monster: &Monster) -> Value {
    json!({
        "id": monster.id.sts_id(),
        "current_hp": monster.hp,
        "max_hp": monster.max_hp,
        "block": monster.block,
        "powers": monster.powers.iter().map(power).collect::<Vec<_>>(),
        "intent": snake_debug(monster.intent),
        "intent_damage": monster.intent_damage,
        "intent_base_damage": monster.intent_base_damage,
        "intent_hits": monster.intent_hits,
        "next_move": monster.next_move,
        "move_history": monster.move_history,
        "dead": monster.dead,
        "escaped": monster.escaped,
        "half_dead": monster.half_dead,
        "alive": monster.alive(),
        "first_move": monster.first_move,
        "extra": monster.extra,
        "stolen_gold": monster.stolen_gold,
        "split_triggered": monster.split_triggered,
        "stasis_card": monster.stasis_card.as_ref().map(card),
        "ascension": monster.ascension,
        "pending_curl": monster.pending_curl,
        "offset_x": monster.offset_x,
        "just_spawned": monster.just_spawned,
    })
}

fn edge(edge: &MapEdge) -> Value {
    json!({
        "source": {"x": edge.src_x, "y": edge.src_y},
        "destination": {"x": edge.dst_x, "y": edge.dst_y},
        "taken": edge.taken,
    })
}

fn node(node: &MapNode) -> Value {
    json!({
        "x": node.x,
        "y": node.y,
        "room": node.room.map(|room| room.simple_name()),
        "taken": node.taken,
        "emerald_key": node.emerald_key,
        "edges": node.edges.iter().map(edge).collect::<Vec<_>>(),
        "parents": node.parents.iter().map(|(x, y)| json!({"x": x, "y": y})).collect::<Vec<_>>(),
    })
}

fn reward(reward: &Reward) -> Value {
    let detail = match reward.kind {
        RewardKind::Gold(amount) => json!({"kind": "gold", "amount": amount}),
        RewardKind::StolenGold(amount) => json!({"kind": "stolen_gold", "amount": amount}),
        RewardKind::Potion(id) => json!({"kind": "potion", "id": id.sts_id()}),
        RewardKind::Relic(id) => json!({"kind": "relic", "id": id.sts_id()}),
        RewardKind::Card => json!({"kind": "card"}),
        RewardKind::EmeraldKey => json!({"kind": "emerald_key"}),
        RewardKind::SapphireKey => json!({"kind": "sapphire_key"}),
    };
    let mut value = detail;
    value["taken"] = json!(reward.taken);
    value["relic_link"] = json!(reward.relic_link);
    value
}

fn sorted(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut values: Vec<_> = values.collect();
    values.sort();
    values
}

impl Game {
    /// Complete, renderer-neutral JSON state at the current decision boundary.
    ///
    /// This deliberately contains no coordinates, textures, animation clocks,
    /// input devices, or other presentation state. Delayed gameplay work is
    /// represented explicitly under `delayed` so a caller never needs to infer
    /// a state transition from a visual-effect object.
    pub fn state_json(&self) -> Value {
        let event = self.event.as_ref().map(|event| {
            json!({
                "id": event.id,
                "screen": event.screen,
                "options": event.options,
                "data": event.data,
                "library_cards": cards(&event.library_cards),
                "match_cards": event.match_cards.iter().map(|entry| json!({
                    "id": entry.id.sts_id(),
                    "flipped": entry.flipped,
                    "revealed": entry.revealed,
                })).collect::<Vec<_>>(),
                "match_chosen": event.match_chosen,
                "match_attempts": event.match_attempts,
            })
        });

        let combat = self.combat.as_ref().map(|combat| {
            json!({
                "encounter": combat.encounter.sts_key(),
                "turn": combat.turn,
                "cards_played_this_turn": combat.cards_played_this_turn,
                "skills_this_turn": combat.skills_this_turn,
                "attacks_this_turn": combat.attacks_this_turn,
                "monsters": combat.monsters.iter().map(monster).collect::<Vec<_>>(),
                "selection": {
                    "need_exhaust": combat.need_exhaust_select,
                    "need_put_on_deck": combat.need_put_on_deck,
                    "need_discard_to_hand": combat.need_discard_to_hand,
                    "need_draw_to_hand": combat.need_draw_to_hand,
                    "need_discovery": combat.need_discovery,
                    "need_forethought": combat.need_forethought,
                },
                "delayed": {
                    "pending_exhaust": combat.pending_exhaust.as_ref().map(card),
                    "draw_after_exhaust": combat.draw_after_exhaust,
                    "pending_dark_embrace": combat.pending_dark_embrace,
                    "pending_ink_bottle": combat.pending_ink_bottle,
                },
                "orbs_channeled_this_combat": combat.orbs_channeled_this_combat
                    .iter().map(|orb| snake_debug(*orb)).collect::<Vec<_>>(),
                "ascension": combat.ascension,
            })
        });

        let grid = self.grid.as_ref().map(|grid| {
            json!({
                "kind": snake_debug(grid.kind),
                "needed": grid.needed,
                "confirm": grid.confirm,
                "hovered": grid.hovered,
                "picked": grid.picked,
                "return_event": grid.return_event,
                "return_shop": grid.return_shop,
                "return_screen": grid.return_screen.map(snake_debug),
                "immediate": grid.immediate,
                "summary": self.grid_summary(),
            })
        });

        let pending_room = self
            .pending_room
            .map(|(x, y, room)| json!({"x": x, "y": y, "room": room.simple_name()}));

        json!({
            "schema_version": 1,
            "game": {
                "seed": self.seed,
                "ascension": self.ascension,
                "character": self.character.sts_name(),
                "done": self.done,
                "final_act_available": self.final_act_available,
            },
            "screen": {
                "name": snake_debug(self.screen),
                "neow_screen": self.neow_screen,
            },
            "room": {
                "type": self.current_room.simple_name(),
                "x": self.current_x,
                "y": self.current_y,
                "pending": pending_room,
            },
            "dungeon": {
                "act": self.dungeon.act as i32,
                "id": self.dungeon.id,
                "name": self.dungeon.name,
                "floor": self.dungeon.floor,
                "boss": self.dungeon.boss,
                "boss_list": self.dungeon.boss_list,
                "monster_list": self.dungeon.monster_list,
                "elite_list": self.dungeon.elite_list,
                "event_list": self.dungeon.event_list,
                "shrine_list": self.dungeon.shrine_list,
                "special_one_time": self.dungeon.special_one_time,
                "relic_pools": {
                    "common": self.dungeon.common_relics,
                    "uncommon": self.dungeon.uncommon_relics,
                    "rare": self.dungeon.rare_relics,
                    "shop": self.dungeon.shop_relics,
                    "boss": self.dungeon.boss_relics,
                },
                "card_pools": {
                    "common": self.dungeon.common_cards.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                    "uncommon": self.dungeon.uncommon_cards.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                    "rare": self.dungeon.rare_cards.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                    "colorless": self.dungeon.colorless_cards.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                    "source_colorless": self.dungeon.src_colorless_cards.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                    "curse": self.dungeon.curse_cards.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                },
                "map": self.dungeon.map.nodes.iter()
                    .map(|row| row.iter().map(node).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
                "path": self.dungeon.path_x.iter().zip(self.dungeon.path_y.iter())
                    .map(|(x, y)| json!({"x": x, "y": y})).collect::<Vec<_>>(),
                "first_room_chosen": self.dungeon.first_room_chosen,
            },
            "player": {
                "current_hp": self.player.hp,
                "max_hp": self.player.max_hp,
                "block": self.player.block,
                "gold": self.player.gold,
                "energy": self.player.energy,
                "energy_master": self.player.energy_master,
                "potion_slots": self.player.potion_slots,
                "relics": self.player.relics.iter().map(|relic| json!({
                    "id": relic.id.sts_id(),
                    "counter": relic.counter,
                    "used_up": relic.used_up,
                })).collect::<Vec<_>>(),
                "potions": self.player.potions.iter().map(|potion| json!({
                    "id": potion.id.sts_id(),
                    "slot": potion.slot,
                })).collect::<Vec<_>>(),
                "powers": self.player.powers.iter().map(power).collect::<Vec<_>>(),
                "master_deck": cards(&self.player.deck),
                "draw_pile": cards(&self.player.draw),
                "hand": cards(&self.player.hand),
                "discard_pile": cards(&self.player.discard),
                "exhaust_pile": cards(&self.player.exhaust),
                "duplication": self.player.duplication,
                "pending_static": self.player.pending_static,
                "orbs": self.player.orbs.iter().map(|orb| json!({
                    "kind": snake_debug(orb.kind),
                    "evoke": orb.evoke,
                })).collect::<Vec<_>>(),
                "max_orbs": self.player.max_orbs,
                "master_max_orbs": self.player.master_max_orbs,
            },
            "combat": combat,
            "event": event,
            "rewards": self.rewards.iter().map(reward).collect::<Vec<_>>(),
            "card_reward": cards(&self.card_reward),
            "neow_options": self.neow_options.iter().map(|option| json!({
                "label": option.label,
                "kind": snake_debug(option.kind),
            })).collect::<Vec<_>>(),
            "boss_relics": self.boss_relics.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
            "shop": {
                "open": self.shop.open,
                "purge_cost": self.shop.purge_cost,
                "purge_available": self.shop.purge_available,
                "cards": self.shop.cards.iter().map(|offer| json!({
                    "item": card(&offer.item), "price": offer.price, "sold": offer.sold,
                })).collect::<Vec<_>>(),
                "relics": self.shop.relics.iter().map(|offer| json!({
                    "id": offer.item.sts_id(), "price": offer.price, "sold": offer.sold,
                })).collect::<Vec<_>>(),
                "potions": self.shop.potions.iter().map(|offer| json!({
                    "id": offer.item.sts_id(), "price": offer.price, "sold": offer.sold,
                })).collect::<Vec<_>>(),
            },
            "keys": {
                "ruby": self.has_ruby_key,
                "emerald": self.has_emerald_key,
                "sapphire": self.has_sapphire_key,
            },
            "selection": {
                "hand_indices": self.hand_select,
                "grid": grid,
                "exhaust": self.exhaust_select,
                "put_on_deck": self.put_on_deck_select,
                "gambling": self.gambling_select,
                "memories": self.memories_select,
                "discovery_combat": self.discovery_combat,
                "discovery_type": self.discovery_typ.map(snake_debug),
                "discovery_colorless": self.discovery_colorless,
            },
            "rest": {
                "smithing": self.rest_smithing,
                "smith_picked": self.rest_smith_picked,
                "selected": self.rest_selected,
            },
            "treasure": {
                "gold": self.chest_gold,
                "gold_amount": self.chest_gold_amt,
                "tier": snake_debug(self.chest_tier),
            },
            "delayed": {
                "cards": cards(&self.pending_cards),
                "gold": self.pending_gold,
                "rest_heal": self.pending_rest_heal,
                "equip_relics": self.pending_equip.iter().map(|id| id.sts_id()).collect::<Vec<_>>(),
                "held_cards": cards(&self.hand_held),
                "shop_purge_index": self.pending_shop_purge,
                "ornithopter_heal": self.pending_ornithopter_heal,
            },
            "chances": {
                "event_elite": self.event_elite_chance,
                "event_monster": self.event_monster_chance,
                "event_shop": self.event_shop_chance,
                "event_treasure": self.event_treasure_chance,
                "potion_blizzard": self.potion_blizzard,
                "card_blizzard": self.card_blizz,
            },
            "rng": {
                "streams": self.rng.snapshot(),
                "neow": self.neow_rng.snapshot(),
            },
            "unlocks": {
                "everything_unlocked": self.unlocks.everything_unlocked,
                "final_act_available": self.unlocks.final_act_available,
                "locked_cards": sorted(self.unlocks.locked_cards.iter().cloned()),
                "locked_relics": sorted(self.unlocks.locked_relics.iter().cloned()),
                "seen_bosses": sorted(self.unlocks.seen_bosses.iter().cloned()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_renderer_neutral_and_complete() {
        let game = Game::new(42, Character::Defect, 20, Unlocks::all());
        let state = game.state_json();
        assert_eq!(state["game"]["seed"], 42);
        assert_eq!(state["game"]["character"], "DEFECT");
        assert_eq!(state["screen"]["name"], "neow");
        assert_eq!(state["player"]["master_deck"].as_array().unwrap().len(), 11);
        assert!(state.get("graphics").is_none());
        assert!(state["rng"]["streams"].is_object());
    }

    #[test]
    fn open_shop_exposes_inventory_and_purchase_actions() {
        let mut game = Game::new(42, Character::Defect, 0, Unlocks::all());
        game.current_room = RoomType::Shop;
        game.open_shop();

        game.step(&Action::Choose {
            index: 0,
            label: Some("shop".into()),
            x: None,
            y: None,
            room: None,
        });

        let state = game.state_json();
        assert_eq!(state["shop"]["open"], true);
        assert!(!state["shop"]["cards"].as_array().unwrap().is_empty());
        assert!(game
            .legal_actions()
            .iter()
            .any(|action| matches!(action, Action::Choose { label: Some(_), .. })));
        assert!(game
            .legal_actions()
            .iter()
            .any(|action| matches!(action, Action::Proceed)));
    }
}
