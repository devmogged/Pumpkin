use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pumpkin_data::{
    configured_feature::ConfiguredFeature as ConfiguredId,
    placed_feature::PlacedFeature as PlacedId,
};

use pumpkin_data::Block;
use pumpkin_util::math::pool::Weighted;
use pumpkin_worldgen_model::feature_data::{
    BlockColumn, ConfiguredFeature as FeatureJson, codec_kind, probability,
};
use serde_json::Value;

use super::{
    block_state_provider::{
        BlockStateProvider, BlockStateRule, PillarBlockStateProvider,
        RandomizedIntBlockStateProvider, RuleBasedBlockStateProvider, SimpleStateProvider,
        WeightedBlockStateProvider,
    },
    feature::{
        configured_features::{ConfiguredFeature, VANILLA_BLOCK_STATE_PROVIDERS_JSON},
        features::{
            bamboo::BambooFeature,
            block_column::{BlockColumnFeature, Layer},
            block_pile::BlockPileFeature,
            fill_layer::FillLayerFeature,
            netherrack_replace_blobs::ReplaceBlobsFeature,
            ore::{OreFeature, OreTarget},
            replace_single_block::ReplaceSingleBlockFeature,
            scattered_ore::ScatteredOreFeature,
            simple_block::SimpleBlockFeature,
        },
        placed_features::{PlacedFeature, PlacedFeatureWrapper},
    },
    rule::{
        RuleTest, block_match::BlockMatchRuleTest, block_state_match::BlockStateMatchRuleTest,
        random_block_match::RandomBlockMatchRuleTest,
        random_block_state_match::RandomBlockStateMatchRuleTest, tag_match::TagMatchRuleTest,
    },
    worldgen_feature_loader::FeatureLoader,
    worldgen_overrides::{read_json, resource_id},
    worldgen_placement::{BlockTags, block_tag, field, predicate, read, state},
};

#[derive(Default)]
pub(super) struct RuntimeFeatures {
    names: BTreeMap<String, PlacedId>,
    placed: BTreeMap<PlacedId, Option<Arc<PlacedFeature>>>,
    configured: BTreeMap<ConfiguredId, Option<Arc<ConfiguredFeature>>>,
}

static RUNTIME_FEATURES: OnceLock<RuntimeFeatures> = OnceLock::new();

impl RuntimeFeatures {
    pub fn id(&self, name: &str) -> Option<PlacedId> {
        self.names.get(resource_id(name).as_ref()).copied()
    }

    pub fn placed(id: &PlacedId) -> Option<&'static Option<Arc<PlacedFeature>>> {
        RUNTIME_FEATURES.get()?.placed.get(id)
    }

    pub fn configured(id: ConfiguredId) -> Option<&'static Option<Arc<ConfiguredFeature>>> {
        RUNTIME_FEATURES.get()?.configured.get(&id)
    }

    pub fn install(self) -> Result<(), String> {
        if self.placed.is_empty() && self.configured.is_empty() {
            return Ok(());
        }
        RUNTIME_FEATURES
            .set(self)
            .map_err(|_| "could not install placed features".to_owned())
    }
}

pub(super) struct FeatureFiles<'a> {
    pub configured: &'a BTreeMap<String, PathBuf>,
    pub placed: &'a BTreeMap<String, PathBuf>,
    pub providers: &'a BTreeMap<String, PathBuf>,
    pub block_tags: &'a BlockTags,
}

impl FeatureFiles<'_> {
    pub fn build(&self) -> RuntimeFeatures {
        let mut loader = FeatureLoader::new(self);
        for key in self.configured.keys() {
            let _ = loader.configured_holder(&Value::String(key.clone()));
        }
        for key in self.placed.keys() {
            let _ = loader.placed_holder(&Value::String(key.clone()));
        }
        let mut result = RuntimeFeatures {
            configured: loader
                .configured
                .into_iter()
                .filter_map(|(name, feature)| {
                    ConfiguredId::from_name(&name).map(|id| (id, feature))
                })
                .collect(),
            ..RuntimeFeatures::default()
        };
        for (name, feature) in loader.placed {
            let id = PlacedId::from_name(&name)
                .unwrap_or_else(|| PlacedId::Runtime(Box::leak(name.clone().into_boxed_str())));
            result.names.insert(name, id);
            result.placed.insert(id, feature);
        }
        result
    }

    pub fn configured(
        &self,
        json: &Value,
        placed: &mut impl FnMut(&Value) -> Result<PlacedFeatureWrapper, String>,
    ) -> Result<ConfiguredFeature, String> {
        let FeatureJson { kind, config } = FeatureJson::parse(json)?;
        Ok(match kind {
            "no_op" => ConfiguredFeature::NoOp,
            "tree" => ConfiguredFeature::Tree(Box::new(super::worldgen_tree::compile(
                config,
                self.block_tags,
                &|value| self.provider(value),
            )?)),
            "simple_block" => ConfiguredFeature::SimpleBlock(SimpleBlockFeature {
                to_place: self.provider(&config["to_place"])?,
                schedule_tick: field(config, "schedule_tick")?,
            }),
            "fill_layer" => ConfiguredFeature::FillLayer(FillLayerFeature {
                height: field(config, "height")?,
                state: state(&config["state"])?,
            }),
            "bamboo" => ConfiguredFeature::Bamboo(BambooFeature {
                probability: probability(config, "probability")?,
            }),
            "block_column" => {
                let column: BlockColumn = read(config)?;
                ConfiguredFeature::BlockColumn(BlockColumnFeature {
                    layers: column
                        .layers
                        .into_iter()
                        .map(|layer| {
                            Ok(Layer {
                                height: layer.height,
                                provider: self.provider(&layer.provider)?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                    direction: read(&Value::String(column.direction))?,
                    allowed_placement: predicate(&column.allowed_placement, self.block_tags)?,
                    prioritize_tip: column.prioritize_tip,
                })
            }
            "block_pile" => ConfiguredFeature::BlockPile(BlockPileFeature {
                state_provider: self.provider(&config["state_provider"])?,
            }),
            "replace_single_block" => {
                ConfiguredFeature::ReplaceSingleBlock(ReplaceSingleBlockFeature {
                    targets: targets(config, self.block_tags)?,
                })
            }
            "netherrack_replace_blobs" => {
                ConfiguredFeature::NetherrackReplaceBlobs(ReplaceBlobsFeature {
                    target: state(&config["target"])?,
                    state: state(&config["state"])?,
                    radius: field(config, "radius")?,
                })
            }
            "ore" | "scattered_ore" => {
                let size = field::<i32>(config, "size")?;
                if !(0..=64).contains(&size) {
                    return Err("ore size must be in 0..=64".into());
                }
                let discard_chance_on_air_exposure =
                    field(config, "discard_chance_on_air_exposure")?;
                let targets = targets(config, self.block_tags)?;
                if kind == "ore" {
                    ConfiguredFeature::Ore(OreFeature {
                        size,
                        discard_chance_on_air_exposure,
                        targets,
                    })
                } else {
                    ConfiguredFeature::ScatteredOre(ScatteredOreFeature {
                        size,
                        discard_chance_on_air_exposure,
                        targets,
                    })
                }
            }
            _ => {
                return super::worldgen_composite_feature::compile(
                    kind,
                    config,
                    self.block_tags,
                    &|provider| self.provider(provider),
                    placed,
                );
            }
        })
    }

    fn provider(&self, value: &Value) -> Result<BlockStateProvider, String> {
        self.decode_provider(value, 0)
    }

    fn decode_provider(&self, value: &Value, depth: usize) -> Result<BlockStateProvider, String> {
        const MAX_PROVIDER_DEPTH: usize = 64;
        if depth >= MAX_PROVIDER_DEPTH {
            return Err("block-state provider nesting exceeds 64".into());
        }
        if let Some(name) = value.as_str() {
            let id = resource_id(name);
            if let Some(path) = self.providers.get(id.as_ref()) {
                return self.decode_provider(&read_json(path)?, depth + 1);
            }
            if let Ok(index) = VANILLA_BLOCK_STATE_PROVIDERS_JSON
                .binary_search_by_key(&id.as_ref(), |&(name, _)| name)
            {
                let json = serde_json::from_str(VANILLA_BLOCK_STATE_PROVIDERS_JSON[index].1)
                    .map_err(|error| format!("{id}: {error}"))?;
                return self.decode_provider(&json, depth + 1);
            }
        }
        if value.get("type").is_none() {
            return Ok(BlockStateProvider::Simple(SimpleStateProvider {
                state: state(value)?,
            }));
        }
        Ok(match codec_kind(value)? {
            "simple" | "simple_state_provider" => BlockStateProvider::Simple(SimpleStateProvider {
                state: state(&value["state"])?,
            }),
            "rotated" | "rotated_block_provider" => {
                BlockStateProvider::Pillar(PillarBlockStateProvider {
                    state: state(&value["state"])?,
                })
            }
            "weighted" | "weighted_state_provider" => {
                let mut total = 0i32;
                let entries = entries(value, "entries")?
                    .iter()
                    .map(|entry| {
                        let weight = field::<i32>(entry, "weight")?;
                        total = total
                            .checked_add(weight)
                            .filter(|_| weight > 0)
                            .ok_or("invalid weighted provider weight")?;
                        Ok(Weighted {
                            data: state(&entry["data"])?,
                            weight,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if entries.is_empty() {
                    return Err("empty weighted block-state provider".into());
                }
                BlockStateProvider::Weighted(WeightedBlockStateProvider { entries })
            }
            "rule_based" | "rule_based_state_provider" => {
                let rules = entries(value, "rules")?
                    .iter()
                    .map(|rule| {
                        Ok(BlockStateRule {
                            if_true: predicate(&rule["if_true"], self.block_tags)?,
                            then: self.decode_provider(&rule["then"], depth + 1)?,
                        })
                    })
                    .collect::<Result<_, String>>()?;
                let fallback = value
                    .get("fallback")
                    .map(|value| self.decode_provider(value, depth + 1).map(Box::new))
                    .transpose()?;
                BlockStateProvider::Rule(RuleBasedBlockStateProvider { fallback, rules })
            }
            "randomized_int" | "randomized_int_state_provider" => {
                let property: String = field(value, "property")?;
                BlockStateProvider::RandomizedInt(RandomizedIntBlockStateProvider {
                    source: Box::new(self.decode_provider(&value["source"], depth + 1)?),
                    property: pumpkin_data::PropertyKey::from_str(&property)
                        .ok_or_else(|| format!("unknown block property {property}"))?,
                    values: field(value, "values")?,
                })
            }
            kind => return super::worldgen_noise_provider::compile(kind, value),
        })
    }
}

fn targets(config: &Value, tags: &BlockTags) -> Result<Vec<OreTarget>, String> {
    entries(config, "targets")?
        .iter()
        .map(|entry| {
            Ok(OreTarget {
                target: rule(&entry["target"], tags)?,
                state: state(&entry["state"])?,
            })
        })
        .collect()
}

fn entries<'a>(value: &'a Value, name: &str) -> Result<&'a [Value], String> {
    value[name]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing or invalid {name}"))
}

fn rule(json: &Value, tags: &BlockTags) -> Result<RuleTest, String> {
    let kind = json["predicate_type"]
        .as_str()
        .ok_or("missing predicate_type")?;
    let kind = kind.strip_prefix("minecraft:").unwrap_or(kind);
    let rules = || {
        entries(json, "rules")?
            .iter()
            .map(|value| rule(value, tags))
            .collect::<Result<_, _>>()
    };
    let block = || {
        let name = json["block"].as_str().ok_or("missing block")?;
        Block::from_name(name)
            .map(|block| block.id)
            .ok_or_else(|| format!("unknown block {name}"))
    };
    Ok(match kind {
        "always_true" => RuleTest::AlwaysTrue,
        "block_match" => RuleTest::BlockMatch(BlockMatchRuleTest { block: block()? }),
        "tag_match" => RuleTest::TagMatch(TagMatchRuleTest {
            tag: block_tag(json["tag"].as_str().ok_or("missing tag")?, tags)?,
        }),
        "blockstate_match" => RuleTest::BlockStateMatch(BlockStateMatchRuleTest {
            block_state: state(&json["block_state"])?.id,
        }),
        "random_block_match" => RuleTest::RandomBlockMatch(RandomBlockMatchRuleTest {
            block: block()?,
            probability: field(json, "probability")?,
        }),
        "random_blockstate_match" => {
            RuleTest::RandomBlockStateMatch(RandomBlockStateMatchRuleTest {
                block_state: state(&json["block_state"])?.id,
                probability: field(json, "probability")?,
            })
        }
        "any_of" => RuleTest::AnyOf(rules()?),
        "all_of" => RuleTest::AllOf(rules()?),
        "not" => RuleTest::Not(Box::new(rule(&json["rule"], tags)?)),
        "height_match" => RuleTest::HeightMatch {
            min_inclusive: field::<Option<i32>>(json, "min_inclusive")?.unwrap_or(i32::MIN),
            max_inclusive: field::<Option<i32>>(json, "max_inclusive")?.unwrap_or(i32::MAX),
        },
        _ => return Err(format!("unsupported rule test {kind}")),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::generation::{get_world_gen, proto_chunk_test::stage_parity::TestBlockRegistry};
    use pumpkin_data::dimension::Dimension;
    use pumpkin_util::{
        math::position::BlockPos,
        random::{RandomGenerator, worldgen_random::WorldgenRandom},
        world_seed::Seed,
    };

    #[test]
    fn rule_based_provider_uses_first_matching_rule_then_fallback() {
        const TAG: &str = "minecraft:base_stone_overworld";
        const REPLACE_WITH: &[u16] = &[Block::DIRT.id.as_u16()];
        let replacement: pumpkin_data::tag::Tag = (&[], REPLACE_WITH, TAG);
        let tags = BlockTags::from([(TAG.into(), replacement)]);
        let files = FeatureFiles {
            configured: &BTreeMap::new(),
            placed: &BTreeMap::new(),
            providers: &BTreeMap::new(),
            block_tags: &tags,
        };
        let config = serde_json::json!({
            "type": "minecraft:simple_block",
            "to_place": {
                "type": "minecraft:rule_based",
                "fallback": {"type": "minecraft:simple", "state": "minecraft:diamond_block"},
                "rules": [
                    {
                        "if_true": {"type": "minecraft:matching_blocks", "blocks": "#minecraft:base_stone_overworld"},
                        "then": {"type": "minecraft:simple", "state": "minecraft:gold_block"}
                    },
                    {
                        "if_true": {"type": "minecraft:matching_blocks", "blocks": ["minecraft:stone", "minecraft:dirt"]},
                        "then": {"type": "minecraft:simple", "state": "minecraft:iron_block"}
                    }
                ]
            }
        });
        let ConfiguredFeature::SimpleBlock(feature) = files
            .configured(&config, &mut |_| Err("unexpected placed holder".into()))
            .unwrap()
        else {
            unreachable!();
        };
        let generator = get_world_gen(
            Seed(42),
            Dimension::OVERWORLD,
            false,
            Vec::new(),
            String::new(),
        );
        let mut chunk = crate::ProtoChunk::new(0, 0, &generator);
        let mut random = RandomGenerator::Worldgen(WorldgenRandom::from_seed(42));
        let pos = BlockPos::new(0, 120, 0);
        for (from, expected) in [
            (Block::STONE, Block::IRON_BLOCK),
            (Block::DIRT, Block::GOLD_BLOCK),
            (Block::AIR, Block::DIAMOND_BLOCK),
        ] {
            chunk.set_block_state(pos.0.x, pos.0.y, pos.0.z, from.default_state);
            assert!(feature.generate(&TestBlockRegistry, &mut chunk, &mut random, pos));
            assert_eq!(chunk.get_block_state(&pos.0).to_block_id(), expected.id);
        }
    }

    #[test]
    fn runtime_oak_matches_generated_oak() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/datapack/data/minecraft/worldgen");
        let files = FeatureFiles {
            configured: &BTreeMap::new(),
            placed: &BTreeMap::new(),
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        };
        let json = read_json(&assets.join("feature/oak.json")).unwrap();
        let ConfiguredFeature::Tree(runtime) = files
            .configured(&json, &mut |_| Err("unexpected placed holder".into()))
            .unwrap()
        else {
            unreachable!();
        };
        let Some(ConfiguredFeature::Tree(generated)) =
            crate::generation::feature::configured_features::CONFIGURED_FEATURES
                .get(&ConfiguredId::Oak)
        else {
            unreachable!();
        };
        let generator = get_world_gen(
            Seed(42),
            Dimension::OVERWORLD,
            false,
            Vec::new(),
            String::new(),
        );
        let place = |tree: &super::super::feature::features::tree::TreeFeature| {
            let mut chunk = crate::ProtoChunk::new(0, 0, &generator);
            chunk.set_block_state(8, 119, 8, Block::GRASS_BLOCK.default_state);
            let mut random = RandomGenerator::Worldgen(WorldgenRandom::from_seed(42));
            assert!(tree.generate(
                &TestBlockRegistry,
                &mut chunk,
                &mut random,
                BlockPos::new(8, 120, 8)
            ));
            chunk.flat_block_map.into_states()
        };
        assert_eq!(place(&runtime), place(generated));
    }

    #[test]
    fn runtime_noise_provider_matches_java_normalization() {
        let files = FeatureFiles {
            configured: &BTreeMap::new(),
            placed: &BTreeMap::new(),
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        };
        let positions = [
            BlockPos::new(1024, 292, 1024),
            BlockPos::new(1027, 293, 1029),
            BlockPos::new(18_001_033, 294, -18_001_038),
        ];
        // Java 26.3 NormalNoise with WorldgenRandom backed by LegacyRandomSource(2345).
        for (normalization, expected) in [
            (
                serde_json::json!(true),
                [0xbe6f_53bb, 0xbeb9_bf1a, 0xbe3c_196e],
            ),
            (
                serde_json::json!(false),
                [0xbed1_6942, 0xbf22_8737, 0xbea4_9641],
            ),
            (
                serde_json::json!("legacy"),
                [0xbe03_29b8, 0xbe4b_98ac, 0xbdce_2ce0],
            ),
        ] {
            let value = serde_json::json!({
                "type": "minecraft:noise",
                "seed": 2345,
                "scale": 0.125,
                "states": ["minecraft:gold_block", "minecraft:iron_block"],
                "noise": {
                    "base_octave": -3,
                    "octave_count": 3,
                    "amplitude_modifiers": [1.0, 0.5, 3.0],
                    "base_amplitude": 0.6,
                    "normalize": normalization
                }
            });
            let BlockStateProvider::NoiseProvider(provider) = files.provider(&value).unwrap()
            else {
                unreachable!();
            };
            assert_eq!(
                positions.map(|pos| provider.base.get_noise(pos).to_bits()),
                expected
            );
        }
    }

    #[test]
    fn runtime_block_column_keeps_tip_at_a_blocked_column() {
        let files = FeatureFiles {
            configured: &BTreeMap::new(),
            placed: &BTreeMap::new(),
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        };
        let json = serde_json::json!({
            "type": "minecraft:block_column",
            "direction": "up",
            "allowed_placement": {"type": "minecraft:matching_blocks", "blocks": "minecraft:air"},
            "prioritize_tip": true,
            "layers": [
                {"height": 3, "provider": {"id":"minecraft:iron_block"}},
                {"height": 1, "provider": {"id":"minecraft:gold_block"}}
            ]
        });
        let ConfiguredFeature::BlockColumn(column) = files
            .configured(&json, &mut |_| Err("unexpected placed holder".into()))
            .unwrap()
        else {
            unreachable!();
        };
        let generator = get_world_gen(
            Seed(42),
            Dimension::OVERWORLD,
            false,
            Vec::new(),
            String::new(),
        );
        let mut chunk = crate::ProtoChunk::new(0, 0, &generator);
        chunk.set_block_state(8, 122, 8, Block::STONE.default_state);
        let mut random = RandomGenerator::Worldgen(WorldgenRandom::from_seed(42));
        assert!(column.generate(
            &mut chunk,
            &TestBlockRegistry,
            -64,
            384,
            PlacedId::Runtime("fixture:column"),
            &mut random,
            BlockPos::new(8, 120, 8)
        ));
        assert_eq!(
            chunk
                .get_block_state(&BlockPos::new(8, 120, 8).0)
                .to_block_id(),
            Block::GOLD_BLOCK.id
        );
        assert!(
            chunk
                .get_block_state(&BlockPos::new(8, 121, 8).0)
                .to_state()
                .is_air()
        );
        assert_eq!(
            chunk
                .get_block_state(&BlockPos::new(8, 122, 8).0)
                .to_block_id(),
            Block::STONE.id
        );
    }

    #[test]
    fn named_runtime_feature_uses_the_origin_chunk_for_fixed_placement() {
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("configured.json");
        let placed = temp.path().join("placed.json");
        std::fs::write(
            &configured,
            r#"{
            "type": "minecraft:simple_block",
            "schedule_tick": true,
            "to_place": {"type": "minecraft:simple", "state": "minecraft:gold_block"}
        }"#,
        )
        .unwrap();
        std::fs::write(
            &placed,
            r#"{
            "feature": "fixture:marker",
            "placement": [{"type": "minecraft:fixed_placement", "positions": [[-7, 120, 2]]}]
        }"#,
        )
        .unwrap();
        let features = FeatureFiles {
            configured: &BTreeMap::from([("fixture:marker".into(), configured)]),
            placed: &BTreeMap::from([("fixture:fixed".into(), placed)]),
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        }
        .build();
        let id = features.id("fixture:fixed").unwrap();
        let feature = features.placed[&id].as_ref().unwrap();
        let generator = get_world_gen(
            Seed(42),
            Dimension::OVERWORLD,
            false,
            Vec::new(),
            String::new(),
        );
        let mut chunk = crate::ProtoChunk::new(-1, 0, &generator);
        let mut random = RandomGenerator::Worldgen(WorldgenRandom::from_seed(42));
        let marker = BlockPos::new(-7, 120, 2);
        assert!(!feature.generate_in_proto_chunk(
            &mut chunk,
            &TestBlockRegistry,
            id,
            &mut random,
            BlockPos::ZERO
        ));
        assert!(chunk.get_block_state(&marker.0).to_state().is_air());
        assert!(feature.generate_in_proto_chunk(
            &mut chunk,
            &TestBlockRegistry,
            id,
            &mut random,
            BlockPos::new(-16, 0, 0)
        ));
        assert_eq!(
            chunk.get_block_state(&marker.0).to_block_id(),
            Block::GOLD_BLOCK.id
        );
        let mut staged = crate::chunk_system::Chunk::Proto(Box::new(chunk));
        staged.upgrade_to_level_chunk(
            &Dimension::OVERWORLD,
            &pumpkin_config::lighting::LightingEngineConfig::Default,
        );
        let crate::chunk_system::Chunk::Level(saved) = staged else {
            unreachable!();
        };
        let resumed = crate::ProtoChunk::from_chunk_data(&saved, &generator);
        assert_eq!(resumed.block_ticks.len(), 1);
        assert_eq!(resumed.block_ticks[0].position, marker);
        assert_eq!(resumed.block_ticks[0].value.id, Block::GOLD_BLOCK.id);
        assert_eq!(resumed.block_ticks[0].delay, 1);
    }
}
