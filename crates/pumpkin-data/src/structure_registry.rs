//! Structures and structure sets from datapacks, layered over the generated vanilla ones.
//!
//! Worldgen installs one registry before it creates generators. Until then every lookup answers
//! with vanilla data.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::structures::{Structure, StructureKeys, StructureSet};

/// The datapack view of the structure registries. Entries are leaked because generators and
/// structure starts keep `'static` references to them for the life of the server.
#[derive(Default)]
pub struct StructureRegistry {
    custom: Vec<(&'static str, &'static Structure)>,
    custom_ids: HashMap<&'static str, u16>,
    overrides: HashMap<StructureKeys, &'static Structure>,
    biomes: HashMap<StructureKeys, &'static [u16]>,
    biome_tags: HashMap<String, &'static [u16]>,
    names: Vec<&'static str>,
    sets: Vec<&'static StructureSet>,
    set_names: Vec<&'static str>,
}

/// Adds `minecraft:` to an ID without a namespace.
#[must_use]
pub fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

impl StructureRegistry {
    /// Starts from the vanilla structure sets and names.
    #[must_use]
    pub fn vanilla() -> Self {
        Self {
            names: StructureKeys::VANILLA_NAMES.to_vec(),
            sets: StructureSet::VANILLA.to_vec(),
            set_names: StructureSet::NAMES.to_vec(),
            ..Self::default()
        }
    }

    /// Reserves the key for a structure ID, giving IDs that vanilla lacks a custom key.
    pub fn key(&mut self, id: &str) -> Option<StructureKeys> {
        let id = full_id(id);
        if let Some(path) = id.strip_prefix("minecraft:")
            && let Some(key) = StructureKeys::vanilla(path)
        {
            return Some(key);
        }
        if let Some(&index) = self.custom_ids.get(id.as_str()) {
            return Some(StructureKeys::Custom(index));
        }
        let index = u16::try_from(self.custom.len()).ok()?;
        let id: &'static str = Box::leak(id.into_boxed_str());
        self.custom.push((id, &UNDEFINED));
        self.custom_ids.insert(id, index);
        self.names.push(id);
        Some(StructureKeys::Custom(index))
    }

    /// Defines or replaces the structure behind `key`.
    pub fn set_structure(&mut self, key: StructureKeys, structure: Structure) {
        let structure: &'static Structure = Box::leak(Box::new(structure));
        match key {
            StructureKeys::Custom(index) => {
                if let Some(entry) = self.custom.get_mut(usize::from(index)) {
                    entry.1 = structure;
                }
            }
            _ => {
                self.overrides.insert(key, structure);
            }
        }
    }

    /// Sets the biome IDs a structure may start in, resolved against the datapack biome tags.
    pub fn set_biomes(&mut self, key: StructureKeys, biomes: Vec<u16>) {
        self.biomes
            .insert(key, Box::leak(biomes.into_boxed_slice()));
    }

    /// Keeps the merged biome tags (`namespace:path` to biome IDs) for placement lookups.
    pub fn set_biome_tags(&mut self, tags: impl IntoIterator<Item = (String, Vec<u16>)>) {
        self.biome_tags = tags
            .into_iter()
            .map(|(name, ids)| (name, &*Box::leak(ids.into_boxed_slice())))
            .collect();
    }

    /// Defines or replaces a structure set. Vanilla sets keep their index.
    pub fn set_structure_set(&mut self, id: &str, set: StructureSet) {
        let id = full_id(id);
        let name = id.strip_prefix("minecraft:").unwrap_or(&id);
        let set: &'static StructureSet = Box::leak(Box::new(set));
        if let Some(index) = self.set_names.iter().position(|existing| *existing == name) {
            self.sets[index] = set;
        } else {
            self.set_names
                .push(Box::leak(name.to_owned().into_boxed_str()));
            self.sets.push(set);
        }
    }

    /// Custom structures that a set references but no file defines.
    #[must_use]
    pub fn undefined(&self) -> Vec<&'static str> {
        self.custom
            .iter()
            .filter(|(_, structure)| std::ptr::eq(*structure, &UNDEFINED))
            .map(|(id, _)| *id)
            .collect()
    }
}

/// The placeholder behind a custom key until its structure file is read.
static UNDEFINED: Structure = Structure::UNDEFINED;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CURRENT: RwLock<Option<&'static StructureRegistry>> = RwLock::new(None);

/// Replaces the active registry.
pub fn install(registry: StructureRegistry) {
    let registry: &'static StructureRegistry = Box::leak(Box::new(registry));
    *CURRENT
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(registry);
    ACTIVE.store(true, Ordering::Release);
}

fn current() -> Option<&'static StructureRegistry> {
    if !ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    *CURRENT
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn custom_key(id: &str) -> Option<StructureKeys> {
    let registry = current()?;
    registry
        .custom_ids
        .get(full_id(id).as_str())
        .map(|&index| StructureKeys::Custom(index))
}

pub(crate) fn custom_name(index: u16) -> &'static str {
    current()
        .and_then(|registry| registry.custom.get(usize::from(index)))
        .map_or("unknown", |(id, _)| id)
}

pub(crate) fn structure(key: StructureKeys) -> Option<&'static Structure> {
    let registry = current()?;
    match key {
        StructureKeys::Custom(index) => registry
            .custom
            .get(usize::from(index))
            .map(|(_, structure)| *structure),
        _ => registry.overrides.get(&key).copied(),
    }
}

/// Biome IDs installed for a structure, when a datapack registry is active.
#[must_use]
pub fn biomes(key: StructureKeys) -> Option<&'static [u16]> {
    current()?.biomes.get(&key).copied()
}

/// A merged biome tag (`namespace:path`, `minecraft:` optional), when a datapack registry is
/// active and has the tag.
#[must_use]
pub fn biome_tag(tag: &str) -> Option<&'static [u16]> {
    current()?.biome_tags.get(full_id(tag).as_str()).copied()
}

pub(crate) fn names() -> Option<&'static [&'static str]> {
    current().map(|registry| registry.names.as_slice())
}

pub(crate) fn sets() -> Option<(&'static [&'static StructureSet], &'static [&'static str])> {
    current().map(|registry| (registry.sets.as_slice(), registry.set_names.as_slice()))
}
