use std::{collections::BTreeMap, sync::Arc};

use pumpkin_data::{
    configured_feature::ConfiguredFeature as ConfiguredId,
    placed_feature::PlacedFeature as PlacedId,
};
use pumpkin_worldgen_model::feature_data::PlacedFeature as PlacedJson;
use serde_json::Value;

use super::{
    feature::{
        configured_features::ConfiguredFeature,
        placed_features::{Feature, PlacedFeature, PlacedFeatureWrapper},
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
    depth: usize,
}

impl<'files, 'data> FeatureLoader<'files, 'data> {
    pub const fn new(files: &'files FeatureFiles<'data>) -> Self {
        Self {
            files,
            configured: BTreeMap::new(),
            placed: BTreeMap::new(),
            depth: 0,
        }
    }

    pub fn configured_holder(&mut self, value: &Value) -> Result<Feature, String> {
        let Some(name) = value.as_str() else {
            return self.decode_configured(value).map(Feature::Inlined);
        };
        let id = resource_id(name);
        let Some(path) = self.files.configured.get(id.as_ref()) else {
            return ConfiguredId::from_name(name)
                .map(Feature::Named)
                .ok_or_else(|| format!("unknown configured feature {id}"));
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
            return PlacedId::from_name(name)
                .map(PlacedFeatureWrapper::Named)
                .ok_or_else(|| format!("unknown placed feature {id}"));
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
