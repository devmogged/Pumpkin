use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Write},
    path::Path,
    sync::OnceLock,
};

use pumpkin_util::math::boundingbox::BoundingBox;
use serde::{Deserialize, Serialize};

use crate::{
    Block, BlockId, BlockState, BlockStateId, Property, block_properties::BlockProperties,
    item::Item,
};

/// A definition to freeze before opening the world's chunk storage.
pub struct BlockDefinition {
    pub name: String,
    pub display_name: String,
    pub entity: Option<String>,
    pub item_carrier: &'static Item,
    /// The first state is the default.
    pub states: Vec<StateDefinition>,
}

/// An item with no block, frozen with the block definitions.
pub struct ItemDefinition {
    pub name: String,
    pub display_name: String,
    /// The vanilla item it copies components from (stack size, food, tool) and that stock clients
    /// see, drawn with the item's own model.
    pub carrier: &'static Item,
}

pub struct StateDefinition {
    pub properties: BTreeMap<String, String>,
    pub physical: &'static BlockState,
    pub carrier: BlockStateId,
    pub carrier_replacement: Option<BlockStateId>,
    pub shape: Vec<BoundingBox>,
}

pub struct StateEntry {
    pub state: &'static BlockState,
    pub block: &'static Block,
    pub carrier: BlockStateId,
    pub properties: BTreeMap<String, String>,
    pub entity: Option<&'static str>,
    pub shape: Vec<BoundingBox>,
}

pub struct ItemEntry {
    pub item: &'static Item,
    pub carrier: &'static Item,
    pub block: Option<&'static Block>,
    pub display_name: &'static str,
}

/// Frozen extensions to the vanilla lookup tables for this server's world.
#[derive(Default)]
pub struct Registry {
    blocks: HashMap<u16, &'static Block>,
    names: HashMap<&'static str, &'static Block>,
    states: HashMap<u16, StateEntry>,
    items: HashMap<u16, ItemEntry>,
    item_names: HashMap<&'static str, &'static Item>,
    entity_names: HashMap<&'static str, &'static str>,
    replacements: HashMap<BlockStateId, BlockStateId>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IdMap {
    blocks: BTreeMap<String, u16>,
    states: BTreeMap<String, u16>,
    items: BTreeMap<String, u16>,
}

static CONTENT: OnceLock<Registry> = OnceLock::new();

/// Internal block-entity marker. Custom entities are named on disk and kept off the wire.
pub const CUSTOM_ENTITY_TYPE: u16 = u16::MAX - 1;

#[must_use]
pub fn entity_name(name: &str) -> Option<&'static str> {
    CONTENT.get()?.entity_names.get(name).copied()
}

#[must_use]
pub(crate) fn replace_carrier(id: BlockStateId) -> BlockStateId {
    CONTENT
        .get()
        .and_then(|registry| registry.replacements.get(&id))
        .copied()
        .unwrap_or(id)
}

impl Registry {
    /// Read the world's id map and reserve new ids before any chunks can be loaded.
    pub fn open(
        path: &Path,
        definitions: &[BlockDefinition],
        item_definitions: &[ItemDefinition],
    ) -> io::Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => return Err(err),
        };
        let mut ids = match &bytes {
            Some(bytes) => serde_json::from_slice(bytes).map_err(io::Error::other)?,
            None => IdMap::default(),
        };
        let mut registry = Self::default();
        let mut block_ids = IdAllocator::new(&mut ids.blocks, BlockId::COUNT)?;
        let mut state_ids = IdAllocator::new(&mut ids.states, BlockStateId::COUNT)?;
        let mut item_ids = IdAllocator::new(&mut ids.items, Item::VANILLA_COUNT)?;

        for def in definitions {
            if !custom_name(&def.name)
                || registry.names.contains_key(def.name.as_str())
                || def.states.is_empty()
                || def.item_carrier.id >= Item::VANILLA_COUNT
                || def.item_carrier == &Item::AIR
            {
                return Err(invalid("invalid custom block definition"));
            }
            let entity = if let Some(name) = &def.entity {
                if !custom_name(name) {
                    return Err(invalid("invalid custom block entity name"));
                }
                Some(
                    if let Some(&stored) = registry.entity_names.get(name.as_str()) {
                        stored
                    } else {
                        let stored: &'static str = Box::leak(name.clone().into_boxed_str());
                        registry.entity_names.insert(stored, stored);
                        stored
                    },
                )
            } else {
                None
            };
            let block_id = block_ids.reserve(&def.name)?;
            let item_id = item_ids.reserve(&def.name)?;
            let mut states = Vec::with_capacity(def.states.len());
            for state in &def.states {
                if state.carrier.as_u16() >= BlockStateId::COUNT {
                    return Err(invalid("custom block carrier must be vanilla"));
                }
                if let Some(replacement) = state.carrier_replacement {
                    if replacement.as_u16() >= BlockStateId::COUNT
                        || replacement == state.carrier
                        || registry
                            .replacements
                            .insert(state.carrier, replacement)
                            .is_some()
                    {
                        return Err(invalid("invalid or duplicate custom block carrier"));
                    }
                }
                let suffix = serde_json::to_string(&state.properties).map_err(io::Error::other)?;
                let raw = state_ids.reserve(&format!("{}{suffix}", def.name))?;
                if states
                    .iter()
                    .any(|entry: &BlockState| entry.id.as_u16() == raw)
                {
                    return Err(invalid("duplicate custom block state"));
                }
                let mut physical = state.physical.clone();
                physical.id = BlockStateId::registered(raw);
                physical.block_entity_type = if entity.is_some() {
                    CUSTOM_ENTITY_TYPE
                } else {
                    u16::MAX
                };
                states.push(physical);
            }

            let name = Box::leak(def.name.clone().into_boxed_str());
            let states = Box::leak(states.into_boxed_slice());
            let Some(default_state) = states.first() else {
                return Err(invalid("missing default block state"));
            };
            let block = Box::leak(Box::new(Block {
                id: BlockId::registered(block_id),
                name,
                item_id,
                default_state,
                states,
                flammable: None,
                experience: None,
                // Vanilla's default block loot table id, so a mod's `data/` can supply drops.
                loot_table: def.name.split_once(':').map(|(namespace, path)| {
                    &*Box::leak(format!("{namespace}:blocks/{path}").into_boxed_str())
                }),
                ..def.states[0].physical.id.to_block().clone()
            }));
            let item = Box::leak(Box::new(Item {
                id: item_id,
                registry_key: name,
                crafting_remainder: None,
                components: def.item_carrier.components,
            }));

            registry.blocks.insert(block_id, block);
            registry.names.insert(name, block);
            registry.item_names.insert(name, item);
            registry.items.insert(
                item_id,
                ItemEntry {
                    item,
                    carrier: def.item_carrier,
                    block: Some(block),
                    display_name: Box::leak(def.display_name.clone().into_boxed_str()),
                },
            );
            for (state, definition) in states.iter().zip(&def.states) {
                registry.states.insert(
                    state.id.as_u16(),
                    StateEntry {
                        state,
                        block,
                        carrier: definition.carrier,
                        properties: definition.properties.clone(),
                        entity,
                        shape: definition.shape.clone(),
                    },
                );
            }
        }

        for def in item_definitions {
            if !custom_name(&def.name)
                || registry.item_names.contains_key(def.name.as_str())
                || def.carrier.id >= Item::VANILLA_COUNT
                || def.carrier == &Item::AIR
            {
                return Err(invalid("invalid custom item definition"));
            }
            let item_id = item_ids.reserve(&def.name)?;
            let name: &'static str = Box::leak(def.name.clone().into_boxed_str());
            let item = Box::leak(Box::new(Item {
                id: item_id,
                registry_key: name,
                crafting_remainder: None,
                components: def.carrier.components,
            }));
            registry.item_names.insert(name, item);
            registry.items.insert(
                item_id,
                ItemEntry {
                    item,
                    carrier: def.carrier,
                    block: None,
                    display_name: Box::leak(def.display_name.clone().into_boxed_str()),
                },
            );
        }

        if registry
            .replacements
            .values()
            .any(|state| registry.replacements.contains_key(state))
        {
            return Err(invalid("carrier replacement is itself reserved"));
        }
        if bytes.is_some() || !definitions.is_empty() || !item_definitions.is_empty() {
            let updated = serde_json::to_vec(&ids).map_err(io::Error::other)?;
            if bytes.as_deref() != Some(updated.as_slice()) {
                let parent = path
                    .parent()
                    .ok_or_else(|| invalid("registry path has no parent"))?;
                fs::create_dir_all(parent)?;
                let draft = path.with_extension("tmp");
                let mut file = fs::File::create(&draft)?;
                file.write_all(&updated)?;
                file.sync_all()?;
                drop(file);
                fs::rename(draft, path)?;
            }
        }
        Ok(registry)
    }

    /// Publish the completed registry once, before opening the worlds.
    pub fn install(self) -> io::Result<()> {
        CONTENT
            .set(self)
            .map_err(|_| invalid("custom content already installed"))
    }

    #[must_use]
    pub fn block(&self, name: &str) -> Option<&'static Block> {
        self.names.get(name).copied()
    }

    #[must_use]
    pub fn state(&self, id: u16) -> Option<&StateEntry> {
        self.states.get(&id)
    }
}

struct IdAllocator<'a> {
    ids: &'a mut BTreeMap<String, u16>,
    next: u32,
}

impl<'a> IdAllocator<'a> {
    fn new(ids: &'a mut BTreeMap<String, u16>, first: u16) -> io::Result<Self> {
        let mut used = std::collections::HashSet::with_capacity(ids.len());
        let mut next = u32::from(first);
        for &id in ids.values() {
            if id < first || !used.insert(id) {
                return Err(invalid("invalid or duplicate saved registry id"));
            }
            next = next.max(u32::from(id) + 1);
        }
        Ok(Self { ids, next })
    }

    fn reserve(&mut self, name: &str) -> io::Result<u16> {
        if let Some(&id) = self.ids.get(name) {
            return Ok(id);
        }
        let id = u16::try_from(self.next).map_err(|_| invalid("custom registry is full"))?;
        self.ids.insert(name.to_owned(), id);
        self.next += 1;
        Ok(id)
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn custom_name(name: &str) -> bool {
    name.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && namespace != "minecraft"
            && !path.is_empty()
            && namespace.bytes().all(identifier_byte)
            && path.bytes().all(|c| identifier_byte(c) || c == b'/')
    })
}

const fn identifier_byte(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.')
}

#[must_use]
pub fn block(name: &str) -> Option<&'static Block> {
    CONTENT.get()?.block(name)
}

#[must_use]
pub fn state(id: u16) -> Option<&'static StateEntry> {
    CONTENT.get()?.state(id)
}

#[must_use]
pub fn item(id: u16) -> Option<&'static ItemEntry> {
    CONTENT.get()?.items.get(&id)
}

pub(crate) fn block_by_id(id: u16) -> Option<&'static Block> {
    CONTENT.get()?.blocks.get(&id).copied()
}

pub(crate) fn block_by_item(id: u16) -> Option<&'static Block> {
    item(id)?.block
}

pub(crate) fn item_by_name(name: &str) -> Option<&'static Item> {
    CONTENT.get()?.item_names.get(name).copied()
}

impl Item {
    /// The vanilla item a stock client sees in place of this one: itself, or a custom item's
    /// carrier.
    #[must_use]
    pub fn vanilla_view(&'static self) -> &'static Self {
        if self.id < Self::VANILLA_COUNT {
            return self;
        }
        item(self.id).map_or(&Self::AIR, |entry| entry.carrier)
    }
}

/// Resolve the exact raw property map used by chunk NBT and custom content.
pub fn state_from_properties<'a>(
    block: &Block,
    properties: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<BlockStateId> {
    let properties: BTreeMap<_, _> = properties.into_iter().collect();
    block.states.iter().find_map(|candidate| {
        let entry = state(candidate.id.as_u16())?;
        entry
            .properties
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .eq(properties.iter().map(|(&k, &v)| (k, v)))
            .then_some(candidate.id)
    })
}

pub(crate) fn properties(block: &Block, id: BlockStateId) -> Option<Box<dyn BlockProperties>> {
    let index = block
        .states
        .iter()
        .position(|candidate| candidate.id == id)?;
    Some(Box::new(ContentProperties(index as u16, id)))
}

pub(crate) fn from_properties(block: &Block, props: &[Property]) -> Box<dyn BlockProperties> {
    let selected = block
        .states
        .iter()
        .position(|candidate| {
            state(candidate.id.as_u16()).is_some_and(|entry| {
                props.iter().all(|prop| {
                    entry
                        .properties
                        .get(prop.key_str())
                        .is_some_and(|v| v == prop.value_str())
                })
            })
        })
        .unwrap_or_default();
    Box::new(ContentProperties(
        selected as u16,
        block.states[selected].id,
    ))
}

struct ContentProperties(u16, BlockStateId);

impl BlockProperties for ContentProperties {
    fn to_index(&self) -> u16 {
        self.0
    }
    fn from_index(index: u16) -> Self {
        Self(index, BlockStateId::AIR)
    }
    fn handles_block_id(id: BlockId) -> bool {
        id.as_u16() >= BlockId::COUNT
    }
    fn to_state_id(&self, block: &Block) -> BlockStateId {
        block
            .states
            .get(usize::from(self.0))
            .unwrap_or(&block.states[0])
            .id
    }
    fn from_state_id(id: BlockStateId, block: &Block) -> Self {
        let index = block
            .states
            .iter()
            .position(|state| state.id == id)
            .unwrap_or_default();
        Self(index as u16, id)
    }
    fn default(block: &Block) -> Self {
        Self(0, block.default_state.id)
    }
    fn to_props(&self) -> Vec<Property> {
        state(self.1.as_u16())
            .into_iter()
            .flat_map(|entry| &entry.properties)
            .filter_map(|(k, v)| Property::from_str_pair(k, v))
            .collect()
    }
    fn from_props(props: &[Property], block: &Block) -> Self {
        Self::from_state_id(from_properties(block, props).to_state_id(block), block)
    }
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
