use crate::entity::player::advancement::predicate::CriterionConditions;
use pumpkin_data::Advancement;
use pumpkin_data::advancement_data::{
    AdvancementCondition, AdvancementCriterion, AdvancementDisplay, AdvancementReward, FrameType,
};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::identifier::Identifier;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tracing::warn;

/// A parsed dynamic advancement definition from a datapack with pre-parsed criteria.
#[derive(Clone, Debug)]
pub struct DynamicAdvancementDefinition {
    pub id: String,
    pub json: String,
    pub criteria: HashMap<String, (String, Arc<CriterionConditions>)>,
}

/// Loads all advancement JSON files from a directory recursively into `registry`.
pub fn load_advancements_from_dir<S: std::hash::BuildHasher>(
    namespace: &str,
    dir: &Path,
    registry: &mut HashMap<String, DynamicAdvancementDefinition, S>,
) -> usize {
    if !dir.is_dir() {
        return 0;
    }
    let before = registry.len();
    load_advancements_recursive(namespace, dir, dir, registry);
    registry.len() - before
}

fn load_advancements_recursive<S: std::hash::BuildHasher>(
    namespace: &str,
    base_dir: &Path,
    current_dir: &Path,
    registry: &mut HashMap<String, DynamicAdvancementDefinition, S>,
) {
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            load_advancements_recursive(namespace, base_dir, &path, registry);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            && let Ok(rel_path) = path.strip_prefix(base_dir)
        {
            let mut stem_path = rel_path.to_string_lossy().to_string();
            if let Some(stem) = stem_path.strip_suffix(".json") {
                stem_path = stem.to_string();
            }
            let stem_path = stem_path.replace('\\', "/");
            let adv_id = format!("{namespace}:{stem_path}");

            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(val) = serde_json::from_str::<Value>(&content) {
                    let mut criteria = HashMap::new();
                    if let Some(crit_obj) = val.get("criteria").and_then(Value::as_object) {
                        for (name, c_val) in crit_obj {
                            let trigger = c_val
                                .get("trigger")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let cond_str = c_val
                                .get("conditions")
                                .map_or_else(|| "{}".to_string(), ToString::to_string);
                            let parsed = Arc::new(CriterionConditions::parse(&trigger, &cond_str));
                            criteria.insert(name.clone(), (trigger, parsed));
                        }
                    }

                    registry.insert(
                        adv_id.clone(),
                        DynamicAdvancementDefinition {
                            id: adv_id,
                            json: content,
                            criteria,
                        },
                    );
                } else {
                    warn!("Failed to parse advancement JSON at {}", path.display());
                }
            }
        }
    }
}

static DATAPACK_CONDITIONS: OnceLock<HashMap<(String, String), Arc<CriterionConditions>>> =
    OnceLock::new();

/// The parsed conditions of a datapack advancement's criterion.
#[must_use]
pub fn datapack_conditions(
    advancement: &Identifier,
    criterion: &str,
) -> Option<Arc<CriterionConditions>> {
    DATAPACK_CONDITIONS
        .get()?
        .get(&(advancement.to_string(), criterion.to_owned()))
        .cloned()
}

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

/// A text component's translation key, or its literal text, which the client shows unchanged
/// when it has no translation for it.
fn text_key(value: Option<&Value>) -> &'static str {
    let text = match value {
        Some(Value::String(text)) => text.as_str(),
        Some(Value::Object(object)) => object
            .get("translate")
            .or_else(|| object.get("text"))
            .and_then(Value::as_str)
            .unwrap_or(""),
        _ => "",
    };
    leak(text)
}

fn place<'a>(
    id: &'a Identifier,
    depth: f32,
    children: &HashMap<&'a Identifier, Vec<&'a Identifier>>,
    row: &mut f32,
    positions: &mut HashMap<Identifier, (f32, f32)>,
) -> f32 {
    let y = match children.get(id) {
        Some(kids) if !kids.is_empty() => {
            let rows: Vec<f32> = kids
                .iter()
                .map(|kid| place(kid, depth + 1.0, children, row, positions))
                .collect();
            f32::midpoint(rows[0], rows[rows.len() - 1])
        }
        _ => {
            *row += 1.0;
            *row - 1.0
        }
    };
    positions.insert(id.clone(), (depth, y));
    y
}

/// Tab positions for the advancements with a display: depth across, one row per leaf.
fn layout(entries: &[(Identifier, Value)]) -> HashMap<Identifier, (f32, f32)> {
    let shown: HashMap<&Identifier, Option<Identifier>> = entries
        .iter()
        .filter(|(_, json)| json.get("display").is_some())
        .map(|(id, json)| {
            let parent = json
                .get("parent")
                .and_then(Value::as_str)
                .and_then(|parent| Identifier::parse(parent).ok());
            (id, parent)
        })
        .collect();
    let mut children: HashMap<&Identifier, Vec<&Identifier>> = HashMap::new();
    let mut roots = Vec::new();
    for (&id, parent) in &shown {
        match parent
            .as_ref()
            .and_then(|parent| shown.get_key_value(parent))
        {
            Some((&parent, _)) => children.entry(parent).or_default().push(id),
            None => roots.push(id),
        }
    }
    roots.sort_by_key(ToString::to_string);
    for list in children.values_mut() {
        list.sort_by_key(ToString::to_string);
    }
    let mut positions = HashMap::new();
    for root in roots {
        let mut row = 0.0;
        place(root, 0.0, &children, &mut row, &mut positions);
    }
    positions
}

fn build_display(display: &Value, position: Option<(f32, f32)>) -> &'static AdvancementDisplay {
    let icon = display.get("icon").and_then(|icon| {
        let item = icon.get("id").or_else(|| icon.get("item"))?.as_str()?;
        let mut nbt = NbtCompound::new();
        nbt.put_string("id", item.to_owned());
        nbt.put_int("count", 1);
        ItemStack::read_item_stack(&nbt)
    });
    let flag = |name, default| {
        display
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    };
    let frame = match display.get("frame").and_then(Value::as_str) {
        Some("challenge") => FrameType::Challenge,
        Some("goal") => FrameType::Goal,
        _ => FrameType::Task,
    };
    let (x, y) = position.unwrap_or_default();
    Box::leak(Box::new(AdvancementDisplay::new(
        text_key(display.get("title")),
        text_key(display.get("description")),
        icon.unwrap_or_else(|| ItemStack::new(1, &Item::PAPER)),
        frame,
        display.get("background").and_then(Value::as_str).map(leak),
        flag("show_toast", true),
        flag("hidden", false),
        flag("announce_to_chat", true),
        x,
        y,
    )))
}

fn leak_list(names: impl Iterator<Item = &'static str>) -> &'static [&'static str] {
    Box::leak(names.collect::<Vec<_>>().into_boxed_slice())
}

fn build(
    id: Identifier,
    json: &Value,
    definition: &DynamicAdvancementDefinition,
    position: Option<(f32, f32)>,
) -> &'static Advancement {
    let mut names: Vec<&String> = definition.criteria.keys().collect();
    names.sort();
    let criteria = leak_list(names.iter().map(|name| leak(name)));
    let criteria_definitions: &'static [AdvancementCriterion] = Box::leak(
        criteria
            .iter()
            .map(|&name| AdvancementCriterion {
                name,
                trigger: leak(&definition.criteria[name].0),
                conditions: AdvancementCondition::Empty,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    // Vanilla `AdvancementRequirements.allOf`: without a list, every criterion is required.
    let requirements: Vec<&'static [&'static str]> = match json.get("requirements") {
        Some(Value::Array(groups)) => groups
            .iter()
            .filter_map(Value::as_array)
            .map(|group| leak_list(group.iter().filter_map(Value::as_str).map(leak)))
            .collect(),
        _ => criteria
            .iter()
            .map(|&name| leak_list(std::iter::once(name)))
            .collect(),
    };
    let experience = json
        .get("rewards")
        .and_then(|rewards| rewards.get("experience"))
        .and_then(Value::as_i64)
        .and_then(|xp| i32::try_from(xp).ok())
        .unwrap_or(0);
    let recipes = leak_list(
        json.get("rewards")
            .and_then(|rewards| rewards.get("recipes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(leak),
    );
    Box::leak(Box::new(Advancement {
        id,
        parent: json
            .get("parent")
            .and_then(Value::as_str)
            .and_then(|parent| Identifier::parse(parent).ok()),
        send_telemetry: false,
        display: json
            .get("display")
            .map(|display| build_display(display, position)),
        reward: Box::leak(Box::new(AdvancementReward {
            experience,
            recipes,
        })),
        requirements: Box::leak(requirements.into_boxed_slice()),
        criteria,
        criteria_definitions,
    }))
}

/// Adds the non-vanilla datapack advancements to the advancement tree, with their parsed
/// criteria. Only the first load registers them; the tree is built once, before players join.
pub fn register_datapack_advancements<S: std::hash::BuildHasher>(
    definitions: &HashMap<String, DynamicAdvancementDefinition, S>,
) -> usize {
    let mut entries: Vec<(Identifier, Value)> = definitions
        .values()
        .filter(|definition| !definition.id.starts_with("minecraft:"))
        .filter_map(|definition| {
            Some((
                Identifier::parse(&definition.id).ok()?,
                serde_json::from_str(&definition.json).ok()?,
            ))
        })
        .collect();
    entries.sort_by_key(|(id, _)| id.to_string());
    let positions = layout(&entries);
    let mut conditions = HashMap::new();
    let advancements: Vec<&'static Advancement> = entries
        .iter()
        .map(|(id, json)| {
            let definition = &definitions[&id.to_string()];
            for (name, (_, parsed)) in &definition.criteria {
                conditions.insert((id.to_string(), name.clone()), parsed.clone());
            }
            build(id.clone(), json, definition, positions.get(id).copied())
        })
        .collect();
    let count = advancements.len();
    if count == 0 || !pumpkin_data::advancement_data::register_datapack_advancements(advancements) {
        return 0;
    }
    let _ = DATAPACK_CONDITIONS.set(conditions);
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datapack_advancement_keeps_recipe_reward_ids() {
        let json = serde_json::json!({
            "rewards": {
                "experience": 5,
                "recipes": ["minecraft:oak_planks", "mod:dragon_sword"]
            }
        });
        let id = Identifier::parse("mod:reward").unwrap();
        let definition = DynamicAdvancementDefinition {
            id: id.to_string(),
            json: json.to_string(),
            criteria: HashMap::new(),
        };
        let advancement = build(id, &json, &definition, None);
        assert_eq!(advancement.reward.experience, 5);
        assert_eq!(
            advancement.reward.recipes,
            ["minecraft:oak_planks", "mod:dragon_sword"]
        );
    }

    #[test]
    fn layout_puts_children_right_of_their_parent() {
        let entry = |id: &str, parent: Option<&str>| {
            let mut json = serde_json::json!({ "display": {} });
            if let Some(parent) = parent {
                json["parent"] = Value::from(parent);
            }
            (Identifier::parse(id).unwrap(), json)
        };
        let entries = [
            entry("mod:root", None),
            entry("mod:a", Some("mod:root")),
            entry("mod:b", Some("mod:root")),
            entry("mod:c", Some("mod:a")),
        ];
        let positions = layout(&entries);
        let at = |id: &str| positions[&Identifier::parse(id).unwrap()];
        assert_eq!(at("mod:c"), (2.0, 0.0));
        assert_eq!(at("mod:b"), (1.0, 1.0));
        assert_eq!(at("mod:a"), (1.0, 0.0));
        assert_eq!(at("mod:root"), (0.0, 0.5));
    }
}
