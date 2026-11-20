use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

use pumpkin_data::{
    Block, BlockState,
    biome::{Parameter, ParameterPoint},
    chunk::DoublePerlinNoiseParameters,
    dimension::Dimension,
    noise_router::{BaseNoiseRouters, VANILLA_DENSITY_JSON},
    noise_settings::{GenerationShapeConfig, NoiseSettings, VANILLA_SETTINGS_JSON},
};
use pumpkin_worldgen_model::{
    noise_parameter::NoiseParameterJson,
    noise_router::parse_noise_router,
    noise_settings::{BlockStateCodecStruct, NoiseSettingsStruct, ParameterStruct},
};
use serde_json::Value;

/// Worldgen settings selected once when the level opens.
pub struct RuntimeNoiseSettings {
    pub(crate) settings: &'static NoiseSettings,
    pub(crate) router: BaseNoiseRouters,
}

/// Merged noise resources from enabled datapacks in priority order.
#[derive(Default)]
pub struct WorldgenOverrides {
    settings: BTreeMap<String, PathBuf>,
    densities: BTreeMap<String, PathBuf>,
    noises: BTreeMap<String, PathBuf>,
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
                for (kind, registry) in [
                    ("noise_settings", &mut overrides.settings),
                    ("density_function", &mut overrides.densities),
                    ("noise", &mut overrides.noises),
                ] {
                    index_json(&worldgen.join(kind), &prefix, registry);
                }
            }
        }
        overrides
    }

    pub(crate) fn settings(
        &self,
        name: Option<&str>,
        dimension: &Dimension,
    ) -> Option<RuntimeNoiseSettings> {
        if self.settings.is_empty() && self.densities.is_empty() && self.noises.is_empty() {
            return None;
        }
        let name = resource_id(name.unwrap_or_else(|| default_settings_id(dimension)));
        if !self.settings.contains_key(name.as_ref())
            && self.densities.is_empty()
            && self.noises.is_empty()
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
        build_settings(name, settings, &densities, &self.noises)
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

fn resource_id(id: &str) -> std::borrow::Cow<'_, str> {
    if id.contains(':') {
        id.into()
    } else {
        format!("minecraft:{id}").into()
    }
}

fn block_state(codec: BlockStateCodecStruct) -> Result<&'static BlockState, String> {
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
    noises: &BTreeMap<String, PathBuf>,
) -> Result<RuntimeNoiseSettings, String> {
    let parsed = parse_noise_router(
        name.strip_prefix("minecraft:").unwrap_or(name),
        &json,
        densities,
    )?;
    let data = NoiseSettingsStruct::from_json(
        name,
        serde_json::from_value(json).map_err(|error| error.to_string())?,
    );
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
    let router = parsed.build_runtime(|id| {
        let full = resource_id(id);
        let vanilla = DoublePerlinNoiseParameters::id_to_parameters(id);
        let Some(path) = noises.get(full.as_ref()) else {
            return vanilla
                .copied()
                .ok_or_else(|| format!("unknown noise {full}"));
        };
        if let Some(&parameters) = resolved_noises.borrow().get(full.as_ref()) {
            return Ok(parameters);
        }
        let data: NoiseParameterJson =
            serde_json::from_value(read_json(path)?).map_err(|error| format!("{full}: {error}"))?;
        let (octave, amplitudes) = data
            .into_runtime_parameters()
            .map_err(|error| format!("{full}: {error}"))?;
        let digest = md5::compute(full.as_bytes()).0;
        let [lo, hi] = [0, 8].map(|i| u64::from_be_bytes(std::array::from_fn(|j| digest[i + j])));
        let parameters = DoublePerlinNoiseParameters::new(
            vanilla.map_or(usize::MAX, |parameters| parameters.id),
            octave,
            Box::leak(amplitudes.into_boxed_slice()),
            lo,
            hi,
            pumpkin_util::math::java_string_hash(&full),
        );
        resolved_noises
            .borrow_mut()
            .insert(full.into_owned(), parameters);
        Ok(parameters)
    })?;
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
    Ok(RuntimeNoiseSettings { settings, router })
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
        }
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
