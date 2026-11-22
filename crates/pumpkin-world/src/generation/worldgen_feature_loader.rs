use std::{collections::BTreeMap, sync::Arc};

use pumpkin_data::{
    configured_feature::ConfiguredFeature as ConfiguredId,
    placed_feature::PlacedFeature as PlacedId,
};
use pumpkin_worldgen_model::feature_data::PlacedFeature as PlacedJson;
use serde_json::Value;

use super::{
    feature::{
        configured_features::{CONFIGURED_FEATURES, ConfiguredFeature},
        placed_features::{Feature, PLACED_FEATURES, PlacedFeature, PlacedFeatureWrapper},
    },
    worldgen_feature::FeatureFiles,
    worldgen_overrides::{read_json, resource_id},
    worldgen_placement::modifier,
};

/// Resolves each datapack entry once and rejects holder cycles before generation.
pub(super) struct FeatureLoader<'files, 'data> {
    files: &'files FeatureFiles<'data>,
    pub configured: BTreeMap<String, Option<Arc<ConfiguredFeature>>>,
    pub placed: BTreeMap<String, Option<Arc<PlacedFeature>>>,
    builtin_configured: BTreeMap<ConfiguredId, bool>,
    builtin_placed: BTreeMap<PlacedId, bool>,
    depth: usize,
}

impl<'files, 'data> FeatureLoader<'files, 'data> {
    pub const fn new(files: &'files FeatureFiles<'data>) -> Self {
        Self {
            files,
            configured: BTreeMap::new(),
            placed: BTreeMap::new(),
            builtin_configured: BTreeMap::new(),
            builtin_placed: BTreeMap::new(),
            depth: 0,
        }
    }

    pub fn configured_holder(&mut self, value: &Value) -> Result<Feature, String> {
        let Some(name) = value.as_str() else {
            return self.decode_configured(value).map(Feature::Inlined);
        };
        let id = resource_id(name);
        let Some(path) = self.files.configured.get(id.as_ref()) else {
            let builtin = ConfiguredId::from_name(name)
                .ok_or_else(|| format!("unknown configured feature {id}"))?;
            self.check_builtin_configured(builtin)?;
            return Ok(Feature::Named(builtin));
        };
        if let Some(entry) = self.configured.get(id.as_ref()) {
            return entry
                .clone()
                .map(Feature::Inlined)
                .ok_or_else(|| format!("unavailable or cyclic configured feature {id}"));
        }

        self.configured.insert(id.to_string(), None);
        match read_json(path).and_then(|json| self.decode_configured(&json)) {
            Ok(feature) => {
                self.configured
                    .insert(id.into_owned(), Some(Arc::clone(&feature)));
                Ok(Feature::Inlined(feature))
            }
            Err(error) => {
                tracing::warn!("Skipping configured feature {id}: {error}");
                Err(format!("unavailable configured feature {id}"))
            }
        }
    }

    pub fn placed_holder(&mut self, value: &Value) -> Result<PlacedFeatureWrapper, String> {
        let Some(name) = value.as_str() else {
            return self.decode_placed(value).map(PlacedFeatureWrapper::Direct);
        };
        let id = resource_id(name);
        let Some(path) = self.files.placed.get(id.as_ref()) else {
            let builtin =
                PlacedId::from_name(name).ok_or_else(|| format!("unknown placed feature {id}"))?;
            self.check_builtin_placed(builtin)?;
            return Ok(PlacedFeatureWrapper::Named(builtin));
        };
        if let Some(entry) = self.placed.get(id.as_ref()) {
            return entry
                .clone()
                .map(PlacedFeatureWrapper::Direct)
                .ok_or_else(|| format!("unavailable or cyclic placed feature {id}"));
        }

        self.placed.insert(id.to_string(), None);
        match read_json(path).and_then(|json| self.decode_placed(&json)) {
            Ok(feature) => {
                self.placed
                    .insert(id.into_owned(), Some(Arc::clone(&feature)));
                Ok(PlacedFeatureWrapper::Direct(feature))
            }
            Err(error) => {
                tracing::warn!("Skipping placed feature {id}: {error}");
                Err(format!("unavailable placed feature {id}"))
            }
        }
    }

    fn check_builtin_configured(&mut self, id: ConfiguredId) -> Result<(), String> {
        if let Some(&valid) = self.builtin_configured.get(&id) {
            return valid.then_some(()).ok_or_else(|| {
                format!("unavailable or cyclic configured feature {}", id.to_name())
            });
        }
        let feature = CONFIGURED_FEATURES
            .get(&id)
            .ok_or_else(|| format!("unavailable configured feature {}", id.to_name()))?;
        self.builtin_configured.insert(id, false);
        self.with_depth(|loader| loader.check_placed_children(feature))?;
        self.builtin_configured.insert(id, true);
        Ok(())
    }

    fn check_builtin_placed(&mut self, id: PlacedId) -> Result<(), String> {
        if let Some(&valid) = self.builtin_placed.get(&id) {
            return valid
                .then_some(())
                .ok_or_else(|| format!("unavailable or cyclic placed feature {}", id.to_name()));
        }
        let feature = PLACED_FEATURES
            .get(&id)
            .ok_or_else(|| format!("unavailable placed feature {}", id.to_name()))?;
        self.builtin_placed.insert(id, false);
        self.with_depth(|loader| loader.check_configured_child(feature))?;
        self.builtin_placed.insert(id, true);
        Ok(())
    }

    fn check_configured_child(&mut self, placed: &PlacedFeature) -> Result<(), String> {
        match &placed.feature {
            Feature::Named(id) => {
                self.configured_holder(&Value::String(id.to_name().into()))?;
                Ok(())
            }
            Feature::Inlined(feature) => {
                self.with_depth(|loader| loader.check_placed_children(feature))
            }
        }
    }

    fn check_placed_children(&mut self, feature: &ConfiguredFeature) -> Result<(), String> {
        use ConfiguredFeature as F;

        let mut check = |placed: &PlacedFeatureWrapper| match placed {
            PlacedFeatureWrapper::Named(id) => {
                self.placed_holder(&Value::String(id.to_name().into()))?;
                Ok(())
            }
            PlacedFeatureWrapper::Direct(placed) => self.check_configured_child(placed),
        };
        match feature {
            F::Sequence(feature) => feature.features.iter().try_for_each(check),
            F::Overlay(feature) => feature.features.iter().try_for_each(check),
            F::RandomSelector(feature) => {
                for option in &feature.features {
                    check(&option.feature)?;
                }
                check(&feature.default)
            }
            F::RandomBooleanSelector(feature) => {
                check(&feature.feature_true)?;
                check(&feature.feature_false)
            }
            F::SimpleRandomSelector(feature) => feature.features.iter().try_for_each(check),
            F::WeightedRandomSelector(feature) => feature
                .features
                .iter()
                .try_for_each(|entry| check(&entry.feature)),
            F::VegetationPatch(feature) => check(&feature.vegetation_feature),
            F::WaterloggedVegetationPatch(feature) => check(&feature.base.vegetation_feature),
            F::SingleBlockPillar(feature) => feature.cap_feature.as_ref().map_or(Ok(()), check),
            F::Flower(feature) | F::NoBonemealFlower(feature) | F::RandomPatch(feature) => {
                self.check_configured_child(&feature.feature)
            }
            F::RootSystem(feature) => self.check_configured_child(&feature.feature),
            _ => Ok(()),
        }
    }

    fn decode_configured(&mut self, value: &Value) -> Result<Arc<ConfiguredFeature>, String> {
        self.with_depth(|loader| {
            loader
                .files
                .configured(value, &mut |placed| loader.placed_holder(placed))
                .map(Arc::new)
        })
    }

    fn decode_placed(&mut self, value: &Value) -> Result<Arc<PlacedFeature>, String> {
        self.with_depth(|loader| {
            let json = PlacedJson::parse(value)?;
            let feature = loader.configured_holder(json.feature)?;
            let placement = json
                .placement
                .iter()
                .map(|modifier_json| modifier(modifier_json, loader.files.block_tags))
                .collect::<Result<_, _>>()?;
            Ok(Arc::new(PlacedFeature { feature, placement }))
        })
    }

    fn with_depth<T>(
        &mut self,
        decode: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        const MAX_FEATURE_DEPTH: usize = 64;
        if self.depth >= MAX_FEATURE_DEPTH {
            return Err("feature holder nesting exceeds 64".into());
        }
        self.depth += 1;
        let result = decode(self);
        self.depth -= 1;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cycle_through_builtin_placed_feature() {
        let root = tempfile::tempdir().unwrap();
        let override_path = root.path().join("oak.json");
        std::fs::write(
            &override_path,
            r#"{"type":"minecraft:sequence","features":["minecraft:oak_checked"]}"#,
        )
        .unwrap();
        let files = FeatureFiles {
            configured: &BTreeMap::from([("minecraft:oak".into(), override_path)]),
            placed: &BTreeMap::new(),
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        };
        let mut loader = FeatureLoader::new(&files);
        assert!(
            loader
                .configured_holder(&Value::String("minecraft:oak".into()))
                .is_err()
        );
        assert!(
            loader
                .configured_holder(&Value::String("minecraft:birch".into()))
                .is_ok()
        );
    }

    #[test]
    fn configured_placed_cycles_fail_without_discarding_independent_features() {
        let root = tempfile::tempdir().unwrap();
        let write = |name, json| {
            let path = root.path().join(name);
            std::fs::write(&path, json).unwrap();
            path
        };
        let configured = BTreeMap::from([
            (
                "fixture:cycle".into(),
                write(
                    "cycle.json",
                    r#"{
                "type": "minecraft:sequence", "features": ["fixture:loop"]
            }"#,
                ),
            ),
            (
                "fixture:good".into(),
                write(
                    "good.json",
                    r#"{
                "type": "minecraft:sequence", "features": ["fixture:marker"]
            }"#,
                ),
            ),
        ]);
        let placed = BTreeMap::from([
            (
                "fixture:loop".into(),
                write(
                    "loop.json",
                    r#"{
                "feature": "fixture:cycle", "placement": []
            }"#,
                ),
            ),
            (
                "fixture:marker".into(),
                write(
                    "marker.json",
                    r#"{
                "feature": {"type": "minecraft:simple_block", "to_place": {"id": "minecraft:gold_block"}},
                "placement": []
            }"#,
                ),
            ),
        ]);
        let files = FeatureFiles {
            configured: &configured,
            placed: &placed,
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        };
        let mut loader = FeatureLoader::new(&files);
        assert!(
            loader
                .configured_holder(&Value::String("fixture:cycle".into()))
                .is_err()
        );
        let Feature::Inlined(good) = loader
            .configured_holder(&Value::String("fixture:good".into()))
            .unwrap()
        else {
            unreachable!();
        };
        let ConfiguredFeature::Sequence(sequence) = good.as_ref() else {
            unreachable!();
        };
        let marker = sequence.features[0].get().unwrap();
        let Feature::Inlined(configured) = &marker.feature else {
            unreachable!();
        };
        assert!(matches!(
            configured.as_ref(),
            ConfiguredFeature::SimpleBlock(_)
        ));
    }
}
