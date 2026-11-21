use std::collections::BTreeSet;

use pumpkin_data::{dimension::Dimension, registry::RegistryEntryData};
use pumpkin_worldgen_model::dimension::DimensionType;

use crate::world_info::Dimensions;

use super::{WorldgenOverrides, read_json, resource_id};

impl WorldgenOverrides {
    /// Resolves startup levels and their Java dimension-type IDs in synced registry order.
    pub fn resolve_dimensions(
        &mut self,
        mut vanilla: Vec<Dimension>,
        saved: &Dimensions,
        synced_types: &[RegistryEntryData],
    ) -> Result<Vec<Dimension>, String> {
        let custom_keys: BTreeSet<_> = saved
            .keys()
            .chain(self.dimensions.keys())
            .filter(|key| {
                !matches!(
                    key.as_str(),
                    "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
                )
            })
            .cloned()
            .collect();
        self.dimension_type_ids.clear();
        for dimension in &mut vanilla {
            let key = dimension.minecraft_name;
            let resolved = self.resolve_level(dimension.id, key, saved, synced_types)?;
            *dimension = resolved;
        }
        for (index, key) in custom_keys.into_iter().enumerate() {
            let id = u8::try_from(index + 4).map_err(|_| "too many dimensions")?;
            match self.resolve_level(id, &key, saved, synced_types) {
                Ok(dimension) => vanilla.push(dimension),
                Err(error) => tracing::warn!("Skipping dimension {key}: {error}"),
            }
        }
        Ok(vanilla)
    }

    fn resolve_level(
        &mut self,
        id: u8,
        key: &str,
        saved: &Dimensions,
        synced_types: &[RegistryEntryData],
    ) -> Result<Dimension, String> {
        let (namespace, name) = key.split_once(':').ok_or("level key has no namespace")?;
        if !valid_component(namespace) || !name.split('/').all(valid_component) {
            return Err(format!("invalid dimension path {key}"));
        }
        let definition = self.dimension(key);
        let definition = definition.as_ref().or_else(|| saved.get(key));
        let dim_type = resource_id(definition.map_or(key, |value| value.dimension_type.as_str()));
        let type_id = synced_types
            .iter()
            .position(|entry| resource_id(&entry.entry_id) == dim_type)
            .and_then(|id| i32::try_from(id).ok())
            .ok_or_else(|| {
                format!("dimension type {dim_type} is absent from the synced registry")
            })?;
        let dimension = if let Some(path) = self.dimension_types.get(dim_type.as_ref()) {
            let definition: DimensionType = serde_json::from_value(read_json(path)?)
                .map_err(|error| format!("{dim_type}: {error}"))?;
            definition.build_level(id, key)?
        } else {
            let mut dimension = Dimension::from_name(dim_type.as_ref())
                .cloned()
                .ok_or_else(|| format!("no data for dimension type {dim_type}"))?;
            dimension.id = id;
            if dimension.minecraft_name != key {
                dimension.minecraft_name = Box::leak(key.to_owned().into_boxed_str());
            }
            dimension
        };
        // shortcut: generation and heightmap storage still require the original narrow Y range.
        if i8::try_from(dimension.min_y).is_err()
            || dimension.min_y + dimension.height <= 0
            || !(256..=511).contains(&dimension.height)
        {
            return Err(format!(
                "dimension {key} exceeds the supported generation height range"
            ));
        }
        self.dimension_type_ids.insert(key.to_owned(), type_id);
        Ok(dimension)
    }
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-.".contains(&byte)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_info::{Dimension as Definition, Generator};

    #[test]
    fn worlds_sharing_one_type_keep_distinct_world_ids_and_names() {
        let saved = ["fixture:first", "fixture:second"]
            .into_iter()
            .map(|key| {
                (
                    key.to_owned(),
                    Definition {
                        dimension_type: "minecraft:overworld".to_owned(),
                        generator: Generator {
                            generator_type: "minecraft:flat".into(),
                            biome_source: None,
                            settings: None,
                        },
                    },
                )
            })
            .collect();
        let reg = [RegistryEntryData {
            entry_id: "minecraft:overworld".into(),
            data: None,
        }];
        let mut worldgen = WorldgenOverrides::default();
        let worlds = worldgen
            .resolve_dimensions(vec![Dimension::OVERWORLD], &saved, &reg)
            .unwrap();
        assert_eq!(worlds[1].minecraft_name, "fixture:first");
        assert_eq!(worlds[2].minecraft_name, "fixture:second");
        assert_ne!(worlds[1].id, worlds[2].id);
        assert_eq!(worldgen.dimension_type_id("fixture:first"), Some(0));
        assert_eq!(worldgen.dimension_type_id("fixture:second"), Some(0));
    }
}
