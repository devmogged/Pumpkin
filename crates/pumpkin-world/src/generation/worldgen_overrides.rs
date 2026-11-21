use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};

use pumpkin_data::{
    Block, BlockState,
    biome::{Biome, Parameter, ParameterPoint},
    carver::CarverConfig,
    chunk::DoublePerlinNoiseParameters,
    dimension::Dimension,
    material_rule::{MaterialRule, VANILLA_MATERIAL_CONDITIONS, VANILLA_MATERIAL_RULES},
    noise_router::{BaseNoiseRouters, VANILLA_DENSITY_JSON},
    noise_settings::{GenerationShapeConfig, NoiseSettings, VANILLA_SETTINGS_JSON},
    registry::RegistryEntryData,
};
use pumpkin_worldgen_model::{
    carver::CarverJson,
    material::{MaterialRegistry, resolve_rule},
    noise_parameter::NoiseParameterJson,
    noise_router::parse_noise_router,
    noise_settings::{
        BlockStateCodecStruct, NoiseSettingsFileStruct, NoiseSettingsStruct, ParameterStruct,
    },
};
use serde_json::Value;

#[path = "worldgen_dimensions.rs"]
mod dimensions;

/// Worldgen settings selected once when the level opens.
pub struct RuntimeNoiseSettings {
    pub(crate) settings: &'static NoiseSettings,
    pub(crate) router: BaseNoiseRouters,
    pub(crate) surface_rule: Option<&'static MaterialRule>,
}

/// Merged worldgen resources from enabled datapacks in priority order.
#[derive(Default)]
pub struct WorldgenOverrides {
    dimensions: BTreeMap<String, PathBuf>,
    dimension_types: BTreeMap<String, PathBuf>,
    dimension_type_ids: BTreeMap<String, i32>,
    biomes: BTreeMap<String, PathBuf>,
    carvers: BTreeMap<String, PathBuf>,
    biome_tags: BTreeMap<String, Vec<u16>>,
    settings: BTreeMap<String, PathBuf>,
    densities: BTreeMap<String, PathBuf>,
    noises: BTreeMap<String, PathBuf>,
    material_rules: BTreeMap<String, PathBuf>,
    material_conditions: BTreeMap<String, PathBuf>,
}

impl WorldgenOverrides {
    /// Indexes the enabled packs. Later packs replace earlier files with the same ID.
    #[must_use]
    pub fn from_packs<'a>(packs: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut overrides = Self::default();
        for pack in packs {
            let Ok(namespaces) = std::fs::read_dir(pack.join("data")) else {
                continue;
            };
            for namespace in namespaces.flatten() {
                let name = namespace.file_name();
                let Some(name) = name.to_str() else { continue };
                let worldgen = namespace.path().join("worldgen");
                let prefix = format!("{name}:");
                index_json(
                    &namespace.path().join("dimension"),
                    &prefix,
                    &mut overrides.dimensions,
                );
                index_json(
                    &namespace.path().join("dimension_type"),
                    &prefix,
                    &mut overrides.dimension_types,
                );
                for (kind, registry) in [
                    ("biome", &mut overrides.biomes),
                    ("carver", &mut overrides.carvers),
                    ("noise_settings", &mut overrides.settings),
                    ("density_function", &mut overrides.densities),
                    ("noise", &mut overrides.noises),
                    ("material_rule", &mut overrides.material_rules),
                    ("material_condition", &mut overrides.material_conditions),
                ] {
                    index_json(&worldgen.join(kind), &prefix, registry);
                }
            }
        }
        overrides
    }

    #[must_use]
    pub fn has_biome_generation_overrides(&self) -> bool {
        !self.biomes.is_empty() || !self.carvers.is_empty()
    }

    /// Saves the merged tags at the biome IDs installed for this world generation run.
    pub fn set_biome_tags(&mut self, tags: BTreeMap<String, Vec<u16>>) {
        self.biome_tags = tags;
    }

    /// Installs biome definitions at the same IDs as the server's synced registry.
    pub fn install_biomes(&self, registry: &[RegistryEntryData]) -> Result<(), String> {
        if registry.len() > usize::from(u8::MAX) + 1 {
            return Err(format!(
                "{} biome entries exceed the engine's 256-entry limit",
                registry.len()
            ));
        }

        let carvers = self.build_carvers();
        let mut unknown = BTreeSet::new();
        let mut carver = |name: &str| {
            let id = resource_id(name);
            if let Some(&loaded) = carvers.get(id.as_ref()) {
                return loaded;
            }
            let built_in = CarverConfig::from_name(&id);
            if built_in.is_none() && unknown.insert(id.into_owned()) {
                tracing::warn!("Skipping unknown carver {name}");
            }
            built_in
        };
        let mut custom = Vec::with_capacity(self.biomes.len());
        for (id, entry) in registry.iter().enumerate() {
            let key = resource_id(&entry.entry_id);
            let vanilla = Biome::vanilla_from_id(id as u8);
            if let Some(vanilla) = vanilla
                && key != resource_id(vanilla.registry_id)
            {
                return Err(format!(
                    "biome sync ID {id} is {key}, expected {}",
                    vanilla.registry_id
                ));
            }

            let Some(path) = self.biomes.get(key.as_ref()) else {
                if let Some(vanilla) = vanilla
                    && vanilla
                        .carvers
                        .iter()
                        .any(|config| carvers.contains_key(config.registry_id))
                {
                    let mut biome = vanilla.clone();
                    biome.carvers = Box::leak(
                        vanilla
                            .carvers
                            .iter()
                            .filter_map(|config| carver(config.registry_id))
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    custom.push(biome);
                }
                continue;
            };
            let name = vanilla.map_or(key.as_ref(), |biome| biome.registry_id);
            match read_json(path).and_then(|json| {
                crate::biome::registry::build_biome(id as u8, name, json, &mut carver)
            }) {
                Ok(biome) => custom.push(biome),
                Err(error) => tracing::warn!("Skipping biome {key}: {error}"),
            }
        }

        if Biome::install_runtime_biomes(registry.len(), custom) {
            Ok(())
        } else {
            Err("could not install synchronized biome IDs".into())
        }
    }

    fn build_carvers(&self) -> BTreeMap<String, Option<&'static CarverConfig>> {
        self.carvers
            .iter()
            .map(|(id, path)| {
                let config = match read_json(path).and_then(CarverJson::parse) {
                    Ok(carver) => Some(&*Box::leak(Box::new(
                        carver.build(Box::leak(id.clone().into_boxed_str())),
                    ))),
                    Err(error) => {
                        tracing::warn!("Skipping carver {id}: {error}");
                        None
                    }
                };
                (id.clone(), config)
            })
            .collect()
    }

    pub(crate) fn dimension_type_id(&self, name: &str) -> Option<i32> {
        self.dimension_type_ids.get(name).copied()
    }

    pub(crate) fn dimension(&self, id: &str) -> Option<crate::world_info::Dimension> {
        let path = self.dimensions.get(resource_id(id).as_ref())?;
        match read_json(path)
            .and_then(|json| serde_json::from_value(json).map_err(|error| error.to_string()))
        {
            Ok(dimension) => Some(dimension),
            Err(error) => {
                tracing::warn!("Skipping dimension {id}: {error}");
                None
            }
        }
    }

    pub(crate) fn settings(
        &self,
        name: Option<&str>,
        dimension: &Dimension,
    ) -> Option<RuntimeNoiseSettings> {
        let name = resource_id(name.unwrap_or_else(|| default_settings_id(dimension)));
        if !self.settings.contains_key(name.as_ref())
            && self.densities.is_empty()
            && self.noises.is_empty()
            && self.material_rules.is_empty()
            && self.material_conditions.is_empty()
        {
            return None;
        }
        match self.build(&name) {
            Ok(settings) => Some(settings),
            Err(error) => {
                tracing::warn!("Skipping worldgen settings {name}: {error}");
                None
            }
        }
    }

    pub(super) fn build(&self, name: &str) -> Result<RuntimeNoiseSettings, String> {
        let settings = if let Some(path) = self.settings.get(name) {
            read_json(path)?
        } else {
            let json = VANILLA_SETTINGS_JSON
                .iter()
                .find_map(|&(id, json)| (id == name).then_some(json))
                .ok_or_else(|| format!("unknown noise settings {name}"))?;
            serde_json::from_str(json).map_err(|error| error.to_string())?
        };

        let mut densities = BTreeMap::new();
        for &(id, json) in VANILLA_DENSITY_JSON {
            if !self.densities.contains_key(id) {
                densities.insert(
                    id.to_owned(),
                    serde_json::from_str(json).map_err(|error| format!("{id}: {error}"))?,
                );
            }
        }
        for (id, path) in &self.densities {
            match read_json(path) {
                Ok(value) => {
                    densities.insert(id.clone(), value);
                }
                Err(error) => tracing::warn!("Skipping density function {id}: {error}"),
            }
        }
        build_settings(name, settings, &densities, self)
    }

    fn material_json(&self, registry: MaterialRegistry, id: &str) -> Result<Value, String> {
        let (datapack, vanilla) = match registry {
            MaterialRegistry::Rule => (&self.material_rules, VANILLA_MATERIAL_RULES),
            MaterialRegistry::Condition => (&self.material_conditions, VANILLA_MATERIAL_CONDITIONS),
        };
        if let Some(path) = datapack.get(id) {
            return read_json(path);
        }
        let json = vanilla
            .binary_search_by_key(&id, |&(name, _)| name)
            .ok()
            .map(|index| vanilla[index].1)
            .ok_or_else(|| format!("unknown {} {id}", registry.directory()))?;
        serde_json::from_str(json).map_err(|error| format!("{id}: {error}"))
    }

    fn surface_rule(
        &self,
        settings: &str,
        json: Value,
        resolve_noise: &impl Fn(&str) -> Result<&'static DoublePerlinNoiseParameters, String>,
    ) -> Option<&'static MaterialRule> {
        let built =
            resolve_rule(json, |registry, id| self.material_json(registry, id)).and_then(|rule| {
                super::worldgen_surface::compile(rule, resolve_noise, &self.biome_tags)
            });
        match built {
            Ok(rule) => Some(Box::leak(Box::new(rule))),
            Err(error) => {
                tracing::warn!("Skipping material rule for {settings}: {error}");
                None
            }
        }
    }
}

pub(super) fn default_settings_id(dimension: &Dimension) -> &'static str {
    if dimension == &Dimension::OVERWORLD {
        "minecraft:overworld"
    } else if dimension == &Dimension::THE_NETHER {
        "minecraft:nether"
    } else {
        "minecraft:end"
    }
}

fn index_json(dir: &Path, prefix: &str, outputs: &mut BTreeMap<String, PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            index_json(&path, &format!("{prefix}{name}/"), outputs);
        } else if kind.is_file()
            && let Some(name) = name.strip_suffix(".json")
        {
            outputs.insert(format!("{prefix}{name}"), path);
        }
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    const MAX_WORLDGEN_JSON_SIZE: u64 = 8 * 1024 * 1024;
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_WORLDGEN_JSON_SIZE + 1)
                .read_to_string(&mut text)
        })
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if text.len() as u64 > MAX_WORLDGEN_JSON_SIZE {
        return Err(format!(
            "{}: worldgen JSON exceeds the size limit",
            path.display()
        ));
    }
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn resource_id(id: &str) -> std::borrow::Cow<'_, str> {
    if id.contains(':') {
        id.into()
    } else {
        format!("minecraft:{id}").into()
    }
}

pub(super) fn block_state(codec: BlockStateCodecStruct) -> Result<&'static BlockState, String> {
    let (name, properties) = match codec {
        BlockStateCodecStruct::Name(name) => (name, None),
        BlockStateCodecStruct::Structured { name, properties } => (name, properties),
    };
    let block = Block::from_name(&name).ok_or_else(|| format!("unknown block {name}"))?;
    let Some(properties) = properties else {
        return Ok(block.default_state);
    };
    let mut state = block
        .properties(block.default_state.id)
        .map_or_else(Vec::new, |props| props.to_props());
    for (key, value) in properties {
        let property = block
            .parse_property(&key, &value)
            .map_err(|error| format!("{name}[{key}={value}]: {error:?}"))?;
        if let Some(default) = state
            .iter_mut()
            .find(|default| default.key() == property.key())
        {
            *default = property;
        }
    }
    block
        .state_from_properties(&state)
        .ok_or_else(|| format!("invalid state for {name}"))
}

fn build_settings(
    name: &str,
    json: Value,
    densities: &BTreeMap<String, Value>,
    overrides: &WorldgenOverrides,
) -> Result<RuntimeNoiseSettings, String> {
    let parsed = parse_noise_router(
        name.strip_prefix("minecraft:").unwrap_or(name),
        &json,
        densities,
    )?;
    let mut source: NoiseSettingsFileStruct =
        serde_json::from_value(json).map_err(|error| error.to_string())?;
    let surface_rule_json = source.material_rule.take().or(source.surface_rule.take());
    let data = NoiseSettingsStruct::from_json(name, source);
    let default_block = block_state(data.default_block)?;
    let default_fluid = block_state(data.default_fluid)?;
    let shape = GenerationShapeConfig {
        min_y: data.shape.min_y,
        height: data.shape.height,
        size_horizontal: data.shape.size_horizontal,
        size_vertical: data.shape.size_vertical,
    };
    if !(1..=4).contains(&shape.size_horizontal)
        || !(1..=4).contains(&shape.size_vertical)
        || shape.height == 0
        || shape.height > 4064
        || !shape.height.is_multiple_of(16)
        || shape.min_y % 16 != 0
        || i32::from(shape.height) + i32::from(shape.min_y) < 0
    {
        return Err("unsupported terrain shape".into());
    }

    let resolved_noises = std::cell::RefCell::new(BTreeMap::new());
    let resolve_noise = |id: &str| {
        let full = resource_id(id);
        let vanilla = DoublePerlinNoiseParameters::id_to_parameters(id);
        let Some(path) = overrides.noises.get(full.as_ref()) else {
            return vanilla.ok_or_else(|| format!("unknown noise {full}"));
        };
        if let Some(&parameters) = resolved_noises.borrow().get(full.as_ref()) {
            return Ok(parameters);
        }
        let data: NoiseParameterJson =
            serde_json::from_value(read_json(path)?).map_err(|error| format!("{full}: {error}"))?;
        let (octave, amplitudes, normalization) = data
            .into_runtime_parameters()
            .map_err(|error| format!("{full}: {error}"))?;
        let digest = md5::compute(full.as_bytes()).0;
        let [lo, hi] = [0, 8].map(|i| u64::from_be_bytes(std::array::from_fn(|j| digest[i + j])));
        let id = vanilla.map_or_else(
            || DoublePerlinNoiseParameters::COUNT + resolved_noises.borrow().len(),
            |parameters| parameters.id,
        );
        let parameters = DoublePerlinNoiseParameters::new(
            id,
            octave,
            Box::leak(amplitudes.into_boxed_slice()),
            lo,
            hi,
            pumpkin_util::math::java_string_hash(&full),
        )
        .with_normalization(normalization);
        let parameters: &'static DoublePerlinNoiseParameters = Box::leak(Box::new(parameters));
        resolved_noises
            .borrow_mut()
            .insert(full.into_owned(), parameters);
        Ok(parameters)
    };
    let router = parsed.build_runtime(|id| resolve_noise(id).copied())?;
    let surface_rule =
        surface_rule_json.and_then(|json| overrides.surface_rule(name, json, &resolve_noise));
    let parameter = |raw: ParameterStruct| {
        let (min, max) = raw.quantized();
        Parameter::new(min, max)
    };
    let spawn_target = data
        .spawn_target
        .into_iter()
        .map(|point| ParameterPoint {
            temperature: parameter(point.temperature),
            humidity: parameter(point.humidity),
            continentalness: parameter(point.continentalness),
            erosion: parameter(point.erosion),
            depth: parameter(point.depth),
            weirdness: parameter(point.weirdness),
            offset: point.offset.quantized().0,
        })
        .collect::<Vec<_>>();
    let settings = Box::leak(Box::new(NoiseSettings {
        aquifers_enabled: data.aquifers_enabled,
        ore_veins_enabled: data.ore_veins_enabled,
        legacy_random_source: data.legacy_random_source,
        sea_level: data.sea_level,
        default_block,
        default_fluid,
        shape,
        spawn_target: Box::leak(spawn_target.into_boxed_slice()),
    }));
    Ok(RuntimeNoiseSettings {
        settings,
        router,
        surface_rule,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::noise_router::BaseNoiseFunctionComponent;

    #[test]
    fn vanilla_runtime_matches_generated_stacks_and_settings() {
        for name in ["overworld", "amplified", "large_biomes", "nether", "end"] {
            let runtime = WorldgenOverrides::default()
                .build(&format!("minecraft:{name}"))
                .unwrap();
            assert_eq!(
                &runtime.router,
                BaseNoiseRouters::from_name(name).unwrap(),
                "{name}"
            );
            assert_eq!(
                runtime.settings,
                NoiseSettings::from_name(name).unwrap(),
                "{name}"
            );
            assert_eq!(
                runtime.surface_rule,
                MaterialRule::from_name(name),
                "{name}"
            );
        }
    }

    #[test]
    fn vanilla_material_registry_rebuilds_all_generated_rules() {
        let registry = WorldgenOverrides::default();
        let noise = |id: &str| {
            DoublePerlinNoiseParameters::id_to_parameters(id)
                .ok_or_else(|| format!("unknown noise {id}"))
        };
        for name in [
            "bedrock_floor",
            "bedrock_roof",
            "end",
            "nether",
            "overworld",
            "overworld_caves",
            "overworld_floating_islands",
        ] {
            let rule = resolve_rule(Value::String(name.into()), |kind, id| {
                registry.material_json(kind, id)
            })
            .unwrap();
            assert_eq!(
                &super::super::worldgen_surface::compile(rule, &noise, &registry.biome_tags)
                    .unwrap(),
                MaterialRule::from_name(name).unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn vanilla_runtime_matches_generated_carver_configs() {
        for &(name, json) in pumpkin_data::carver::VANILLA_CARVERS_JSON {
            let runtime = CarverJson::parse(serde_json::from_str(json).unwrap())
                .unwrap()
                .build(name);
            let generated = CarverConfig::from_name(name).unwrap();
            assert_eq!(format!("{runtime:?}"), format!("{generated:?}"), "{name}");
        }
    }

    #[test]
    fn higher_pack_replaces_a_named_carver() {
        let packs = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        for (pack, probability) in packs.iter().zip([0.25, 0.75]) {
            let folder = pack.path().join("data/minecraft/worldgen/carver");
            std::fs::create_dir_all(&folder).unwrap();
            let json = pumpkin_data::carver::VANILLA_CARVERS_JSON
                .iter()
                .find_map(|&(id, json)| (id == "minecraft:canyon").then_some(json))
                .unwrap();
            let mut def: Value = serde_json::from_str(json).unwrap();
            def["probability"] = Value::from(probability);
            std::fs::write(folder.join("canyon.json"), def.to_string()).unwrap();
        }
        let registry = WorldgenOverrides::from_packs(packs.iter().map(tempfile::TempDir::path));
        let carvers = registry.build_carvers();
        assert_eq!(carvers["minecraft:canyon"].unwrap().probability, 0.75);
    }

    #[test]
    fn higher_pack_replaces_a_referenced_surface_rule() {
        use crate::generation::generator::VanillaGenerator;
        use pumpkin_data::material_rule::{MaterialCondition, MaterialRule};

        let lower = tempfile::tempdir().unwrap();
        let higher = tempfile::tempdir().unwrap();
        let write = |root: &Path, path: &str, contents: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        };
        let path = "data/minecraft/worldgen/material_rule/overworld.json";
        write(lower.path(), path, "\"fixture:surface\"");
        let path = "data/fixture/worldgen/material_rule/surface.json";
        write(
            lower.path(),
            path,
            r#"{"type":"minecraft:block","result_state":"minecraft:gold_block"}"#,
        );
        write(
            higher.path(),
            path,
            r#"{"type":"minecraft:condition","if_true":"fixture:in_desert","then_run":{"type":"minecraft:block","result_state":{"id":"minecraft:oak_log","properties":{"axis":"x"}}}}"#,
        );
        write(
            lower.path(),
            "data/fixture/worldgen/material_condition/in_desert.json",
            r##"{"type":"minecraft:biome","biome_is":"#fixture:dry"}"##,
        );

        let mut registry = WorldgenOverrides::from_packs([lower.path(), higher.path()]);
        registry.set_biome_tags(BTreeMap::from([(
            "fixture:dry".into(),
            vec![u16::from(Biome::DESERT.id)],
        )]));
        let runtime = registry.settings(None, &Dimension::OVERWORLD);
        let generator = VanillaGenerator::new_with_runtime_settings(
            pumpkin_util::world_seed::Seed(42),
            Dimension::OVERWORLD,
            None,
            None,
            Some(&[]),
            runtime.as_ref(),
        );
        let MaterialRule::Condition(condition) = generator.surface_rule else {
            panic!("expected the datapack's conditional rule");
        };
        let MaterialCondition::Biome(biomes) = &condition.if_true else {
            panic!("expected the datapack's biome condition");
        };
        assert_eq!(biomes.biome_is, &[&Biome::DESERT]);
        let MaterialRule::Block(block) = condition.then_run else {
            panic!("expected the datapack's block rule");
        };
        assert_eq!(
            Block::from_state_id(block.result_state.id).id,
            Block::OAK_LOG.id
        );
        let props = Block::OAK_LOG.properties(block.result_state.id).unwrap();
        assert!(
            props
                .to_props()
                .iter()
                .any(|property| { property.key_str() == "axis" && property.value_str() == "x" })
        );
    }
    #[test]
    fn priority_replaces_dimension_biome_source_and_survives_save() {
        let dirs = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let path = "data/minecraft/dimension/overworld.json";
        let mut dimension = serde_json::json!({
            "type": "minecraft:overworld",
            "generator": {
                "type": "minecraft:noise",
                "settings": "minecraft:overworld",
                "biome_source": { "type": "minecraft:fixed", "biome": "minecraft:plains" }
            }
        });
        for dir in &dirs {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, dimension.to_string()).unwrap();
            dimension["generator"]["biome_source"] = serde_json::json!({
                "type": "minecraft:multi_noise",
                "biomes": [{
                    "biome": "minecraft:desert",
                    "parameters": {
                        "temperature": 0.0,
                        "humidity": [-1.0, 0],
                        "continentalness": { "min": -1.2, "max": 1.0 },
                        "erosion": 0.0,
                        "depth": 0.0,
                        "weirdness": 0.0,
                        "offset": 0.0
                    }
                }]
            });
        }
        let overrides = WorldgenOverrides::from_packs(dirs.iter().map(tempfile::TempDir::path));
        let definition = overrides.dimension("overworld").unwrap();
        let definition_id = "minecraft:overworld";
        let mut settings = crate::world_info::WorldGenSettings::default();
        settings.dimensions.insert(definition_id.into(), definition);
        crate::world_info::data_files::write_world_gen_settings(dirs[0].path(), &settings, 5023)
            .unwrap();
        let reloaded =
            crate::world_info::data_files::read_world_gen_settings(dirs[0].path()).unwrap();
        assert_eq!(settings, reloaded);
        let supplier = crate::biome::ActiveBiomeSupplier::from_biome_source(
            reloaded.dimensions[definition_id]
                .generator
                .biome_source
                .as_ref(),
            &Dimension::OVERWORLD,
        );
        assert_eq!(
            supplier.possible_biomes(),
            std::iter::once(u16::from(pumpkin_data::biome::Biome::DESERT.id)).collect()
        );
    }

    #[test]
    fn pack_priority_and_namespaced_density_references() {
        let lower = tempfile::tempdir().unwrap();
        let higher = tempfile::tempdir().unwrap();
        let write = |root: &Path, id: &str, contents: &str| {
            let path = root.join(id);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        };
        write(
            lower.path(),
            "data/test/worldgen/density_function/terrain.json",
            "-1",
        );
        write(
            higher.path(),
            "data/test/worldgen/density_function/terrain.json",
            "1",
        );
        let json = VANILLA_SETTINGS_JSON
            .iter()
            .find(|entry| entry.0 == "minecraft:overworld")
            .unwrap()
            .1;
        let mut json: Value = serde_json::from_str(json).unwrap();
        json["noise_router"]["final_density"] = Value::from("test:terrain");
        json["sea_level"] = Value::from(47);
        write(
            lower.path(),
            "data/test/worldgen/noise_settings/settings.json",
            &json.to_string(),
        );
        let registry = WorldgenOverrides::from_packs([lower.path(), higher.path()]);
        let runtime = registry
            .settings(Some("test:settings"), &Dimension::OVERWORLD)
            .unwrap();
        assert_eq!(runtime.settings.sea_level, 47);
        assert!(matches!(
            runtime.router.noise.full_component_stack[runtime.router.noise.final_density],
            BaseNoiseFunctionComponent::Constant { value: 1.0 }
        ));
    }
}
