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
    block_state::HAS_ANALOG_OUTPUT_SIGNAL, data_component::DataComponent,
    data_component_impl::DataComponentImpl, item::Item,
};

/// A definition to freeze before opening the world's chunk storage.
pub struct BlockDefinition {
    pub name: String,
    pub display_name: String,
    pub entity: Option<String>,
    pub item_carrier: &'static Item,
    pub material: Option<BlockMaterial>,
    /// The first state is the default.
    pub states: Vec<StateDefinition>,
}

/// Material values independent of a content block's physical state templates.
pub struct BlockMaterial {
    pub template: &'static Block,
    pub hardness: Option<f32>,
    pub map_color: Option<u8>,
}

/// An item with no block, frozen with the block definitions.
pub struct ItemDefinition {
    pub name: String,
    pub display_name: String,
    /// The vanilla item it copies components from (stack size, food, tool) and that stock clients
    /// see, drawn with the item's own model.
    pub carrier: &'static Item,
    pub components: Vec<(DataComponent, Box<dyn DataComponentImpl>)>,
    /// Stock-client defaults that have no effect on the server's item behaviour.
    pub client_components: Vec<(DataComponent, Box<dyn DataComponentImpl>)>,
}

pub struct StateDefinition {
    pub properties: BTreeMap<String, String>,
    pub physical: &'static BlockState,
    pub random_ticks: bool,
    pub carrier: BlockStateId,
    pub carrier_replacement: Option<BlockStateId>,
    pub shape: Vec<BoundingBox>,
}

/// Explicit physics independent of the vanilla state used as a template.
#[derive(Clone, Copy, Debug)]
pub struct BlockPhysics {
    pub hardness: f32,
    pub blast_resistance: f32,
    pub tool_required: bool,
    pub sound_type: u8,
}

impl BlockPhysics {
    #[must_use]
    pub fn valid(self) -> bool {
        self.hardness.is_finite()
            && (self.hardness == -1.0 || self.hardness >= 0.0)
            && self.blast_resistance.is_finite()
            && self.blast_resistance >= 0.0
            && usize::from(self.sound_type) < crate::block_properties::SOUND_TYPES.len()
    }
}

pub struct StateEntry {
    pub state: &'static BlockState,
    pub comparator_output: Option<u8>,
    pub block: &'static Block,
    pub carrier: BlockStateId,
    pub block_display_carrier: Option<BlockStateId>,
    pub properties: BTreeMap<String, String>,
    pub entity: Option<&'static str>,
    pub shape: Vec<BoundingBox>,
}

pub struct ItemEntry {
    pub item: &'static Item,
    pub carrier: &'static Item,
    pub wire_defaults: &'static [(DataComponent, &'static dyn DataComponentImpl)],
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
        Self::open_with_comparator_outputs(path, definitions, item_definitions, |_, _| None)
    }

    /// Supplies fixed analog outputs by block name and index in its ordered state definitions.
    pub fn open_with_comparator_outputs(
        path: &Path,
        definitions: &[BlockDefinition],
        item_definitions: &[ItemDefinition],
        comparator_output: impl Fn(&str, usize) -> Option<u8>,
    ) -> io::Result<Self> {
        Self::open_with_properties(
            path,
            definitions,
            item_definitions,
            comparator_output,
            |_| None,
        )
    }

    /// Freeze custom block physics and state comparator outputs together.
    pub fn open_with_properties(
        path: &Path,
        definitions: &[BlockDefinition],
        item_definitions: &[ItemDefinition],
        comparator_output: impl Fn(&str, usize) -> Option<u8>,
        physics: impl Fn(&str) -> Option<BlockPhysics>,
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
            let override_physics = physics(&def.name);
            if override_physics.is_some_and(|p| !p.valid()) {
                return Err(invalid("invalid custom block physics"));
            }
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
            let outputs: Vec<_> = (0..def.states.len())
                .map(|index| comparator_output(&def.name, index))
                .collect();
            for (state, output) in def.states.iter().zip(&outputs) {
                if output.is_some_and(|strength| strength > 15) {
                    return Err(invalid("invalid comparator output"));
                }
                if state.carrier.as_u16() >= BlockStateId::COUNT {
                    return Err(invalid("custom block carrier must be vanilla"));
                }
                if let Some(replacement) = state.carrier_replacement {
                    Self::reserve_carrier(&mut registry.replacements, state.carrier, replacement)?;
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
                if let Some(material) = &def.material {
                    physical.copy_material(material.template.default_state);
                    if let Some(hardness) = material.hardness {
                        physical.hardness = hardness;
                    }
                }
                if let Some(p) = override_physics {
                    physical.hardness = p.hardness;
                    physical.sound_type = p.sound_type;
                    physical.set_tool_required(p.tool_required);
                }
                physical.id = BlockStateId::registered(raw);
                physical.set_random_ticks(state.random_ticks);
                if output.is_some() {
                    physical.state_flags |= HAS_ANALOG_OUTPUT_SIGNAL;
                }
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
            let block_template = def.material.as_ref().map_or_else(
                || def.states[0].physical.id.to_block(),
                |material| material.template,
            );
            let block = Box::leak(Box::new(Block {
                id: BlockId::registered(block_id),
                name,
                item_id,
                default_state,
                states,
                hardness: override_physics.map_or_else(
                    || {
                        def.material
                            .as_ref()
                            .map_or(block_template.hardness, |_| default_state.hardness)
                    },
                    |p| p.hardness,
                ),
                blast_resistance: override_physics
                    .map_or(block_template.blast_resistance, |p| p.blast_resistance),
                flammable: None,
                experience: None,
                // Vanilla's default block loot table id, so a mod's `data/` can supply drops.
                loot_table: def.name.split_once(':').map(|(namespace, path)| {
                    &*Box::leak(format!("{namespace}:blocks/{path}").into_boxed_str())
                }),
                map_color: def
                    .material
                    .as_ref()
                    .and_then(|material| material.map_color)
                    .unwrap_or(block_template.map_color),
                ..block_template.clone()
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
                    wire_defaults: &[],
                    block: Some(block),
                    display_name: Box::leak(def.display_name.clone().into_boxed_str()),
                },
            );
            for ((state, definition), comparator_output) in
                states.iter().zip(&def.states).zip(outputs)
            {
                registry.states.insert(
                    state.id.as_u16(),
                    StateEntry {
                        state,
                        comparator_output,
                        block,
                        carrier: definition.carrier,
                        block_display_carrier: None,
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
            let defaults = &*Box::leak(
                def.components
                    .iter()
                    .map(|(kind, value)| (*kind, &*Box::leak(value.clone_dyn())))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let components = if defaults.is_empty() {
                def.carrier.components
            } else {
                let components = def
                    .carrier
                    .components
                    .iter()
                    .copied()
                    .filter(|(kind, _)| !defaults.iter().any(|(overridden, _)| overridden == kind))
                    .chain(defaults.iter().copied())
                    .collect::<Vec<_>>();
                &*Box::leak(components.into_boxed_slice())
            };
            let wire_defaults = if def.client_components.is_empty() {
                defaults
            } else {
                &*Box::leak(
                    defaults
                        .iter()
                        .copied()
                        .filter(|(kind, _)| {
                            !def.client_components
                                .iter()
                                .any(|(client, _)| client == kind)
                        })
                        .chain(
                            def.client_components
                                .iter()
                                .map(|(kind, value)| (*kind, &*Box::leak(value.clone_dyn()))),
                        )
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                )
            };
            let name: &'static str = Box::leak(def.name.clone().into_boxed_str());
            let item = Box::leak(Box::new(Item {
                id: item_id,
                registry_key: name,
                crafting_remainder: None,
                components,
            }));
            registry.item_names.insert(name, item);
            registry.items.insert(
                item_id,
                ItemEntry {
                    item,
                    carrier: def.carrier,
                    wire_defaults,
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

    /// Give a registered block state a separate baked carrier for block displays.
    pub fn set_block_display_carrier(
        &mut self,
        id: BlockStateId,
        carrier: BlockStateId,
        replacement: BlockStateId,
    ) -> io::Result<()> {
        let entry = self
            .states
            .get_mut(&id.as_u16())
            .filter(|entry| entry.block_display_carrier.is_none())
            .ok_or_else(|| invalid("invalid or duplicate block display state"))?;
        Self::reserve_carrier(&mut self.replacements, carrier, replacement)?;
        entry.block_display_carrier = Some(carrier);
        Ok(())
    }

    fn reserve_carrier(
        replacements: &mut HashMap<BlockStateId, BlockStateId>,
        carrier: BlockStateId,
        replacement: BlockStateId,
    ) -> io::Result<()> {
        if carrier.as_u16() >= BlockStateId::COUNT
            || replacement.as_u16() >= BlockStateId::COUNT
            || replacement == carrier
            || replacements
                .get(&carrier)
                .is_some_and(|&previous| previous != replacement)
        {
            return Err(invalid("invalid or duplicate custom block carrier"));
        }
        replacements.insert(carrier, replacement);
        Ok(())
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
