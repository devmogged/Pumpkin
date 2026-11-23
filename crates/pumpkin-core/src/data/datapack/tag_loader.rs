use std::collections::HashMap;
use std::fs;
use std::path::Path;

use pumpkin_data::Block;
use pumpkin_data::item::Item;
use pumpkin_data::tag::{RegistryKey, get_latest_map, get_tag_ids, get_tag_references};
use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DynamicTagEntry {
    pub id: String,
    pub values: Vec<String>,
    /// A pack set `replace`, so lower packs and the generated vanilla entries don't count.
    pub replace: bool,
}

#[derive(Deserialize, Debug)]
struct RawTagFile {
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    values: Vec<RawTagValue>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum RawTagValue {
    String(String),
    Object { id: String },
}

pub fn load_tags_from_dir<S1: std::hash::BuildHasher + Default, S2: std::hash::BuildHasher>(
    namespace: &str,
    tags_dir: &Path,
    tags: &mut HashMap<String, HashMap<String, DynamicTagEntry, S1>, S2>,
) -> usize {
    let mut total_loaded = 0;
    let Ok(entries) = fs::read_dir(tags_dir) else {
        return 0;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let cat_name = entry.file_name().to_string_lossy().to_string();
        // Packs older than 1.21 use plural folder names.
        let cat_name = match cat_name.as_str() {
            "blocks" => "block".to_owned(),
            "items" => "item".to_owned(),
            "fluids" => "fluid".to_owned(),
            "entity_types" => "entity_type".to_owned(),
            "game_events" => "game_event".to_owned(),
            "functions" => "function".to_owned(),
            _ => cat_name,
        };
        if cat_name == "worldgen" {
            // Worldgen tags are nested, e.g. tags/worldgen/biome
            if let Ok(wg_entries) = fs::read_dir(&path) {
                for wg_entry in wg_entries.flatten() {
                    let wg_path = wg_entry.path();
                    if wg_path.is_dir() {
                        let sub_cat =
                            format!("worldgen/{}", wg_entry.file_name().to_string_lossy());
                        let category_map = tags.entry(sub_cat).or_default();
                        let before = category_map.len();
                        load_tag_category_recursive(namespace, &wg_path, &wg_path, category_map);
                        total_loaded += category_map.len() - before;
                    }
                }
            }
        } else {
            let category_map = tags.entry(cat_name).or_default();
            let before = category_map.len();
            load_tag_category_recursive(namespace, &path, &path, category_map);
            total_loaded += category_map.len() - before;
        }
    }

    total_loaded
}

/// Hands the loaded block and item tag memberships to [`pumpkin_data::tag_overlay`].
///
/// Only memberships beyond the generated vanilla tags go there.
pub fn publish_overlay<S1: std::hash::BuildHasher, S2: std::hash::BuildHasher>(
    tags: &HashMap<String, HashMap<String, DynamicTagEntry, S1>, S2>,
) {
    let registries: [(&str, RegistryKey, IdLookup); 2] = [
        ("block", RegistryKey::Block, |name| {
            Block::from_registry_key(name).map(|b| b.id.as_u16())
        }),
        ("item", RegistryKey::Item, |name| {
            Item::from_registry_key(name).map(|i| i.id)
        }),
    ];
    let mut memberships: HashMap<(RegistryKey, u16), Vec<String>> = HashMap::new();
    for (category, key, lookup) in registries {
        let Some(category_tags) = tags.get(category) else {
            continue;
        };
        for name in tag_names(category_tags, key) {
            let mut ids = Vec::new();
            resolve_tag(category_tags, key, lookup, name, 0, &mut ids);
            let generated = get_tag_ids(key, name).unwrap_or_default();
            for id in ids {
                if generated.contains(&id) {
                    continue;
                }
                let joined = memberships.entry((key, id)).or_default();
                if !joined.iter().any(|joined| joined == name) {
                    joined.push(name.to_owned());
                }
            }
        }
    }
    pumpkin_data::tag_overlay::set(memberships);
}

type IdLookup = fn(&str) -> Option<u16>;

/// Every tag a registry has once packs load: the loaded ones and the generated vanilla ones,
/// which can gain members through a nested tag a pack extends.
pub(super) fn tag_names<S: std::hash::BuildHasher>(
    tags: &HashMap<String, DynamicTagEntry, S>,
    key: RegistryKey,
) -> impl Iterator<Item = &str> {
    tags.keys().map(String::as_str).chain(
        get_latest_map(key)
            .keys()
            .copied()
            .filter(|name| !tags.contains_key(*name)),
    )
}

/// Vanilla rejects deeper or cyclic references; this only has to stop.
const MAX_TAG_DEPTH: u8 = 32;

pub(super) fn resolve_tag<S: std::hash::BuildHasher>(
    tags: &HashMap<String, DynamicTagEntry, S>,
    key: RegistryKey,
    lookup: IdLookup,
    name: &str,
    depth: u8,
    out: &mut Vec<u16>,
) {
    if depth > MAX_TAG_DEPTH {
        return;
    }
    let entry = tags.get(name);
    if !entry.is_some_and(|entry| entry.replace)
        && let Some(ids) = get_tag_ids(key, name)
    {
        out.extend_from_slice(ids);
        // The generated members are flattened; follow the vanilla references so a pack that
        // extends a nested tag reaches this one too.
        for nested in get_tag_references(key, name) {
            resolve_tag(tags, key, lookup, nested, depth + 1, out);
        }
    }
    let Some(entry) = entry else {
        return;
    };
    for value in &entry.values {
        if let Some(nested) = value.strip_prefix('#') {
            resolve_tag(tags, key, lookup, nested, depth + 1, out);
        } else if let Some(id) = lookup(value) {
            out.push(id);
        }
    }
}

fn load_tag_category_recursive<S: std::hash::BuildHasher>(
    namespace: &str,
    base_dir: &Path,
    current_dir: &Path,
    category_map: &mut HashMap<String, DynamicTagEntry, S>,
) {
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            load_tag_category_recursive(namespace, base_dir, &path, category_map);
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
            let tag_id = format!("{namespace}:{stem_path}");

            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(raw) = serde_json::from_str::<RawTagFile>(&content)
            {
                let mut string_values = Vec::with_capacity(raw.values.len());
                for val in raw.values {
                    match val {
                        RawTagValue::String(s) => string_values.push(s),
                        RawTagValue::Object { id } => string_values.push(id),
                    }
                }

                if raw.replace {
                    category_map.insert(
                        tag_id.clone(),
                        DynamicTagEntry {
                            id: tag_id,
                            values: string_values,
                            replace: true,
                        },
                    );
                } else {
                    let entry =
                        category_map
                            .entry(tag_id.clone())
                            .or_insert_with(|| DynamicTagEntry {
                                id: tag_id,
                                values: Vec::new(),
                                replace: false,
                            });
                    for val in string_values {
                        if !entry.values.contains(&val) {
                            entry.values.push(val);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_load_tags() {
        let temp = tempfile::tempdir().expect("tempdir");
        let tags_dir = temp.path().join("tags");
        let block_tags = tags_dir.join("block");
        fs::create_dir_all(&block_tags).expect("create dir");
        fs::write(
            block_tags.join("custom_tag.json"),
            r#"{"replace": false, "values": ["minecraft:stone", {"id": "minecraft:dirt"}]}"#,
        )
        .expect("write");

        let mut map: HashMap<String, HashMap<String, DynamicTagEntry>> = HashMap::new();
        let loaded = load_tags_from_dir("custom", &tags_dir, &mut map);
        assert_eq!(loaded, 1);
        let block_map = map.get("block").expect("block tags");
        let entry = block_map.get("custom:custom_tag").expect("custom_tag");
        assert_eq!(entry.values, ["minecraft:stone", "minecraft:dirt"]);
    }

    #[test]
    fn loaded_tags_reach_gameplay_tag_checks() {
        use pumpkin_data::tag::Taggable;

        let temp = tempfile::tempdir().expect("tempdir");
        let tags_dir = temp.path().join("tags");
        // A pre-1.21 folder name, nesting a generated tag and a loaded one.
        let block_tags = tags_dir.join("blocks");
        fs::create_dir_all(&block_tags).expect("create dir");
        fs::write(
            block_tags.join("things.json"),
            r##"{"values": ["#minecraft:planks", "#custom:more"]}"##,
        )
        .expect("write");
        fs::write(
            block_tags.join("more.json"),
            r#"{"values": ["minecraft:dirt"]}"#,
        )
        .expect("write");

        let mut map: HashMap<String, HashMap<String, DynamicTagEntry>> = HashMap::new();
        load_tags_from_dir("custom", &tags_dir, &mut map);
        publish_overlay(&map);

        assert_eq!(
            Block::OAK_PLANKS.is_tagged_with("#custom:things"),
            Some(true)
        );
        assert_eq!(Block::DIRT.is_tagged_with("custom:things"), Some(true));
        assert_eq!(Block::STONE.is_tagged_with("custom:things"), None);
        assert_eq!(Block::DIRT.is_tagged_with("minecraft:planks"), Some(false));
    }
}
