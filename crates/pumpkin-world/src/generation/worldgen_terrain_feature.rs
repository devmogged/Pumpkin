//! Runtime codecs for terrain and cave features whose engine types carry their full config.

use std::ops::RangeInclusive;

use pumpkin_data::{Block, BlockId};
use pumpkin_util::math::int_provider::{IntProvider, NormalIntProvider, UniformIntProvider};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    block_state_provider::BlockStateProvider,
    feature::{
        configured_features::ConfiguredFeature,
        features::{
            fallen_tree::FallenTreeFeature,
            geode::GeodeFeature,
            huge_fungus::HugeFungusFeature,
            multiface_growth::MultifaceGrowthFeature,
            sculk_patch::SculkPatchFeature,
            spring_feature::{BlockWrapper, SpringFeatureFeature},
            underwater_magma::UnderwaterMagmaFeature,
            vines::VinesFeature,
        },
    },
    worldgen_placement::{BlockTags, block_tag, bounded_int, field, predicate, state, state_codec},
    worldgen_tree::decorator,
};

pub(super) fn compile(
    kind: &str,
    config: &Value,
    tags: &BlockTags,
    provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
) -> Result<ConfiguredFeature, String> {
    Ok(match kind {
        "vines" => ConfiguredFeature::Vines(VinesFeature),
        "spring_feature" => ConfiguredFeature::SpringFeature(SpringFeatureFeature {
            state: fluid_state(&config["state"])?,
            requires_block_below: optional(config, "requires_block_below", true)?,
            rock_count: optional(config, "rock_count", 4)?,
            hole_count: optional(config, "hole_count", 1)?,
            valid_blocks: holder_names(&config["valid_blocks"], tags)?,
        }),
        "underwater_magma" => ConfiguredFeature::UnderwaterMagma(UnderwaterMagmaFeature {
            floor_search_range: int(config, "floor_search_range", None, 0..=512)?,
            placement_radius: int(config, "placement_radius_around_floor", None, 0..=64)?,
            placement_probability: chance(config, "placement_probability_per_valid_position")?,
        }),
        "sculk_patch" => ConfiguredFeature::SculkPatch(SculkPatchFeature {
            charge_count: int(config, "charge_count", None, 1..=32)?,
            amount_per_charge: int(config, "amount_per_charge", None, 1..=500)?,
            spread_attempts: int(config, "spread_attempts", None, 1..=64)?,
            growth_rounds: int(config, "growth_rounds", None, 0..=8)?,
            spread_rounds: int(config, "spread_rounds", None, 0..=8)?,
        }),
        "huge_fungus" => ConfiguredFeature::HugeFungus(HugeFungusFeature {
            valid_base_block: state(&config["valid_base_block"])?,
            stem_state: state(&config["stem_state"])?,
            hat_state: state(&config["hat_state"])?,
            decor_state: state(&config["decor_state"])?,
            replaceable_blocks: predicate(&config["replaceable_blocks"], tags)?,
            planted: optional(config, "planted", false)?,
        }),
        "fallen_tree" => {
            let decorators = |key: &str| {
                config[key]
                    .as_array()
                    .ok_or_else(|| format!("missing or invalid {key}"))?
                    .iter()
                    .map(|value| decorator(value, provider))
                    .collect::<Result<Vec<_>, String>>()
            };
            ConfiguredFeature::FallenTree(FallenTreeFeature {
                trunk_provider: provider(&config["trunk_provider"])?,
                log_length: bounded_int(config, "log_length", 0..=16)?,
                stump_decorators: decorators("stump_decorators")?,
                log_decorators: decorators("log_decorators")?,
            })
        }
        "multiface_growth" => {
            let name: String = field(config, "block")?;
            let block = Block::from_name(&name).ok_or_else(|| format!("unknown block {name}"))?;
            ConfiguredFeature::MultifaceGrowth(MultifaceGrowthFeature {
                place_block: block.id,
                search_range: int(config, "search_range", Some(10), 1..=64)?,
                can_place_on_floor: optional(config, "can_place_on_floor", false)?,
                can_place_on_ceiling: optional(config, "can_place_on_ceiling", false)?,
                can_place_on_wall: optional(config, "can_place_on_wall", false)?,
                chance_of_spreading: optional_chance(config, "chance_of_spreading", 0.5)?,
                can_be_placed_on: holder_ids(&config["can_be_placed_on"], tags)?,
            })
        }
        "geode" => ConfiguredFeature::Geode(Box::new(geode(config, tags, provider)?)),
        _ => return Err(format!("unsupported configured feature type {kind}")),
    })
}

fn geode(
    config: &Value,
    tags: &BlockTags,
    provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
) -> Result<GeodeFeature, String> {
    const LAYER: RangeInclusive<f64> = 0.01..=50.0;
    const CHANCE: RangeInclusive<f64> = 0.0..=1.0;
    let blocks = &config["blocks"];
    let layers = &config["layers"];
    let crack = &config["crack"];
    let inner_placements = blocks["inner_placements"]
        .as_array()
        .filter(|placements| !placements.is_empty())
        .ok_or("inner_placements must be a non-empty list")?
        .iter()
        .map(state_codec)
        .collect::<Result<_, String>>()?;
    Ok(GeodeFeature {
        filling_provider: provider(&blocks["filling_provider"])?,
        inner_layer_provider: provider(&blocks["inner_layer_provider"])?,
        alternate_inner_layer_provider: provider(&blocks["alternate_inner_layer_provider"])?,
        middle_layer_provider: provider(&blocks["middle_layer_provider"])?,
        outer_layer_provider: provider(&blocks["outer_layer_provider"])?,
        inner_placements,
        cannot_replace: holder_names(&blocks["cannot_replace"], tags)?,
        invalid_blocks: holder_names(&blocks["invalid_blocks"], tags)?,
        filling: ranged(layers, "filling", 1.7, LAYER)?,
        inner_layer: ranged(layers, "inner_layer", 2.2, LAYER)?,
        middle_layer: ranged(layers, "middle_layer", 3.2, LAYER)?,
        outer_layer: ranged(layers, "outer_layer", 4.2, LAYER)?,
        generate_crack_chance: ranged(crack, "generate_crack_chance", 1.0, CHANCE)?,
        base_crack_size: ranged(crack, "base_crack_size", 2.0, 0.0..=5.0)?,
        crack_point_offset: int(crack, "crack_point_offset", Some(2), 0..=10)?,
        use_potential_placements_chance: ranged(
            config,
            "use_potential_placements_chance",
            0.35,
            CHANCE,
        )?,
        use_alternate_layer0_chance: ranged(config, "use_alternate_layer0_chance", 0.0, CHANCE)?,
        placements_require_layer0_alternate: optional(
            config,
            "placements_require_layer0_alternate",
            true,
        )?,
        outer_wall_distance: uniform_or(config, "outer_wall_distance", 4, 5, 1..=20)?,
        distribution_points: uniform_or(config, "distribution_points", 3, 4, 1..=20)?,
        point_offset: uniform_or(config, "point_offset", 1, 2, 0..=10)?,
        min_gen_offset: optional(config, "min_gen_offset", -16)?,
        max_gen_offset: optional(config, "max_gen_offset", 16)?,
        noise_multiplier: ranged(config, "noise_multiplier", 0.05, CHANCE)?,
        invalid_blocks_threshold: field(config, "invalid_blocks_threshold")?,
    })
}

/// A source fluid state as Java places it (Fluid.createLegacyBlock). Springs only use sources.
fn fluid_state(value: &Value) -> Result<&'static pumpkin_data::BlockState, String> {
    let name: String = field(value, "id")?;
    let block = match name.strip_prefix("minecraft:").unwrap_or(&name) {
        "water" => &Block::WATER,
        "lava" => &Block::LAVA,
        _ => return Err(format!("unsupported spring fluid {name}")),
    };
    Ok(block.default_state)
}

fn optional<T: DeserializeOwned>(value: &Value, key: &str, default: T) -> Result<T, String> {
    Ok(field::<Option<T>>(value, key)?.unwrap_or(default))
}

fn int(
    value: &Value,
    key: &str,
    default: Option<i32>,
    range: RangeInclusive<i32>,
) -> Result<i32, String> {
    let value = field::<Option<i32>>(value, key)?
        .or(default)
        .ok_or_else(|| format!("missing {key}"))?;
    range
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("{key} must be within {range:?}"))
}

fn ranged(
    value: &Value,
    key: &str,
    default: f64,
    range: RangeInclusive<f64>,
) -> Result<f64, String> {
    let value = optional(value, key, default)?;
    range
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("{key} must be within {range:?}"))
}

fn chance(value: &Value, key: &str) -> Result<f32, String> {
    let value: f32 = field(value, key)?;
    (0.0..=1.0)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("{key} must be within 0..=1"))
}

fn optional_chance(value: &Value, key: &str, default: f32) -> Result<f32, String> {
    if value.get(key).is_none() {
        return Ok(default);
    }
    chance(value, key)
}

fn uniform_or(
    value: &Value,
    key: &str,
    min_inclusive: i32,
    max_inclusive: i32,
    range: RangeInclusive<i32>,
) -> Result<IntProvider, String> {
    if value.get(key).is_none() {
        return Ok(IntProvider::Object(NormalIntProvider::Uniform(
            UniformIntProvider {
                min_inclusive,
                max_inclusive,
            },
        )));
    }
    bounded_int(value, key, range)
}

/// A block holder set: one name, a list of names, or a `#` tag.
fn holder_ids(value: &Value, tags: &BlockTags) -> Result<Vec<BlockId>, String> {
    if let Some(tag) = value.as_str().filter(|name| name.starts_with('#')) {
        return Ok(block_tag(tag, tags)?
            .1
            .iter()
            .filter_map(|&id| BlockId::new(id))
            .collect());
    }
    let names: Vec<String> = match value.as_str() {
        Some(name) => vec![name.to_owned()],
        None => serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
    };
    names
        .iter()
        .map(|name| {
            Block::from_name(name)
                .map(|block| block.id)
                .ok_or_else(|| format!("unknown block {name}"))
        })
        .collect()
}

fn holder_names(value: &Value, tags: &BlockTags) -> Result<BlockWrapper, String> {
    Ok(BlockWrapper::Multi(
        holder_ids(value, tags)?
            .into_iter()
            .map(|id| Block::from_id(id).name.to_owned())
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use pumpkin_data::{
        configured_feature::ConfiguredFeature as ConfiguredId, dimension::Dimension,
        placed_feature::PlacedFeature as PlacedId,
    };
    use pumpkin_util::{
        math::position::BlockPos,
        random::{RandomGenerator, worldgen_random::WorldgenRandom},
        world_seed::Seed,
    };
    use pumpkin_worldgen_model::feature_data::codec_kind;

    use crate::chunk_system::chunk_state::StagedChunkEnum;
    use crate::generation::{
        feature::configured_features::{CONFIGURED_FEATURES, ConfiguredFeature},
        generator::WorldGenerator,
        get_world_gen,
        proto_chunk::ProtoChunk,
        proto_chunk_test::stage_parity::TestBlockRegistry,
        worldgen_feature::FeatureFiles,
        worldgen_overrides::read_json,
    };

    const KINDS: &[&str] = &[
        "vines",
        "spring_feature",
        "underwater_magma",
        "sculk_patch",
        "huge_fungus",
        "fallen_tree",
        "multiface_growth",
        "geode",
        // Kelp exercises the randomized_int state provider.
        "block_column",
    ];

    fn place(
        feature: &ConfiguredFeature,
        world_gen: &WorldGenerator,
        chunk_x: i32,
        y: i32,
    ) -> (bool, Vec<pumpkin_data::BlockStateId>) {
        let WorldGenerator::Noise(generator) = world_gen else {
            unreachable!()
        };
        let mut chunk = ProtoChunk::new(chunk_x, 0, world_gen);
        chunk.step_to_biomes(generator);
        chunk.stage = StagedChunkEnum::StructureReferences;
        chunk.step_to_noise(generator);
        let mut random = RandomGenerator::Worldgen(WorldgenRandom::from_seed(42));
        let (min_y, height) = (chunk.bottom_y(), chunk.height());
        let placed = feature.generate(
            &mut chunk,
            &TestBlockRegistry,
            min_y,
            height,
            PlacedId::from_name("minecraft:amethyst_geode").unwrap(),
            &mut random,
            BlockPos::new(chunk_x * 16 + 8, y, 8),
        );
        (placed, chunk.flat_block_map)
    }

    #[test]
    fn runtime_terrain_features_match_generated() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/datapack/data/minecraft/worldgen/feature");
        let files = FeatureFiles {
            configured: &BTreeMap::new(),
            placed: &BTreeMap::new(),
            providers: &BTreeMap::new(),
            block_tags: &BTreeMap::new(),
        };
        let world_gen = get_world_gen(
            Seed(42),
            Dimension::OVERWORLD,
            false,
            Vec::new(),
            String::new(),
        );
        let mut checked = 0;
        let mut placements = 0;
        for entry in std::fs::read_dir(assets).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let json = read_json(&path).unwrap();
            if !KINDS.contains(&codec_kind(&json).unwrap()) {
                continue;
            }
            let name = format!("minecraft:{}", path.file_stem().unwrap().to_str().unwrap());
            // Built-in fallen trees drop the shelf mushroom decorator Pumpkin lacks; datapacks reject it.
            let runtime =
                match files.configured(&json, &mut |_| Err("unexpected placed holder".into())) {
                    Ok(runtime) => runtime,
                    Err(error) if error.contains("shelf_mushroom") => continue,
                    Err(error) => panic!("{name}: {error}"),
                };
            let generated = CONFIGURED_FEATURES
                .get(&ConfiguredId::from_name(&name).unwrap())
                .unwrap();
            for (chunk_x, y) in [(0, -40), (0, 10), (1, 40), (2, 64), (3, 90)] {
                let expected = place(generated, &world_gen, chunk_x, y);
                let actual = place(&runtime, &world_gen, chunk_x, y);
                let diff: Vec<_> = actual
                    .1
                    .iter()
                    .zip(&expected.1)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| {
                        (
                            i,
                            pumpkin_data::BlockState::from_id(*a).id,
                            pumpkin_data::Block::from_state_id(*a).name,
                            pumpkin_data::Block::from_state_id(*b).name,
                        )
                    })
                    .take(5)
                    .collect();
                assert_eq!(
                    actual, expected,
                    "{name} at {y}: placed {} vs {}, diff {diff:?}",
                    actual.0, expected.0
                );
                placements += usize::from(expected.0);
            }
            checked += 1;
        }
        assert!(checked >= 15, "checked {checked} vanilla features");
        assert!(placements > 0);
    }
}
