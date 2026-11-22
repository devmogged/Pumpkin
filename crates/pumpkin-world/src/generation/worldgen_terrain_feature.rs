//! Runtime codecs for terrain and cave features whose engine types carry their full config.

use std::ops::RangeInclusive;

use pumpkin_data::{Block, BlockId};
use pumpkin_util::math::{
    float_provider::FloatProvider,
    int_provider::{IntProvider, NormalIntProvider, UniformIntProvider},
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    block_state_provider::BlockStateProvider,
    feature::{
        configured_features::ConfiguredFeature,
        features::{
            drip_stone::{cluster::DripstoneClusterFeature, large::LargeDripstoneFeature},
            fallen_tree::FallenTreeFeature,
            forest_rock::ForestRockFeature,
            geode::GeodeFeature,
            huge_brown_mushroom::{HugeBrownMushroomFeature, HugeMushroom},
            huge_fungus::HugeFungusFeature,
            huge_red_mushroom::HugeRedMushroomFeature,
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
        "block_blob" => ConfiguredFeature::ForestRock(ForestRockFeature {
            state: state(&config["state"])?,
            can_place_on: predicate(&config["can_place_on"], tags)?,
        }),
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
        "huge_brown_mushroom" => ConfiguredFeature::HugeBrownMushroom(HugeBrownMushroomFeature(
            huge_mushroom(config, tags, provider)?,
        )),
        "huge_red_mushroom" => ConfiguredFeature::HugeRedMushroom(HugeRedMushroomFeature(
            huge_mushroom(config, tags, provider)?,
        )),
        "geode" => ConfiguredFeature::Geode(Box::new(geode(config, tags, provider)?)),
        "speleothem_cluster" | "dripstone_cluster" => {
            ConfiguredFeature::DripstoneCluster(speleothem_cluster(config, tags)?)
        }
        "large_dripstone" => ConfiguredFeature::LargeDripstone(LargeDripstoneFeature {
            replaceable_blocks: holder_names(&config["replaceable_blocks"], tags)?,
            floor_to_ceiling_search_range: int(
                config,
                "floor_to_ceiling_search_range",
                Some(30),
                1..=512,
            )?,
            column_radius: bounded_int(config, "column_radius", 1..=16)?,
            height_scale: float(config, "height_scale", 0.0..=20.0)?,
            max_column_radius_to_cave_height_ratio: real(
                config,
                "max_column_radius_to_cave_height_ratio",
                0.1..=1.0,
            )?,
            stalactite_bluntness: float(config, "stalactite_bluntness", 0.1..=10.0)?,
            stalagmite_bluntness: float(config, "stalagmite_bluntness", 0.1..=10.0)?,
            wind_speed: float(config, "wind_speed", 0.0..=2.0)?,
            min_radius_for_wind: int(config, "min_radius_for_wind", None, 0..=100)?,
            min_bluntness_for_wind: real(config, "min_bluntness_for_wind", 0.0..=5.0)?,
        }),
        _ => return Err(format!("unsupported configured feature type {kind}")),
    })
}

fn huge_mushroom(
    config: &Value,
    tags: &BlockTags,
    provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
) -> Result<HugeMushroom, String> {
    Ok(HugeMushroom {
        cap_provider: provider(&config["cap_provider"])?,
        stem_provider: provider(&config["stem_provider"])?,
        foliage_radius: optional(config, "foliage_radius", 2)?,
        can_place_on: predicate(&config["can_place_on"], tags)?,
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

fn speleothem_cluster(config: &Value, tags: &BlockTags) -> Result<DripstoneClusterFeature, String> {
    // The codec reads block states; the feature only uses their blocks.
    let block = |key: &str| state(&config[key]).map(|state| Block::from_state_id(state.id));
    Ok(DripstoneClusterFeature {
        base_block: block("base_block")?,
        pointed_block: block("pointed_block")?,
        replaceable_blocks: holder_names(&config["replaceable_blocks"], tags)?,
        floor_to_ceiling_search_range: int(config, "floor_to_ceiling_search_range", None, 1..=512)?,
        height: bounded_int(config, "height", 1..=128)?,
        radius: bounded_int(config, "radius", 1..=128)?,
        max_stalagmite_stalactite_height_diff: int(
            config,
            "max_stalagmite_stalactite_height_diff",
            None,
            0..=64,
        )?,
        height_deviation: int(config, "height_deviation", None, 1..=64)?,
        speleothem_block_layer_thickness: bounded_int(
            config,
            "speleothem_block_layer_thickness",
            0..=128,
        )?,
        density: float(config, "density", 0.0..=2.0)?,
        wetness: float(config, "wetness", 0.0..=2.0)?,
        chance_of_speleothem_at_max_distance_from_center: chance(
            config,
            "chance_of_speleothem_at_max_distance_from_center",
        )?,
        max_distance_from_edge_affecting_chance_of_speleothem: int(
            config,
            "max_distance_from_edge_affecting_chance_of_speleothem",
            None,
            1..=64,
        )?,
        max_distance_from_center_affecting_height_bias: int(
            config,
            "max_distance_from_center_affecting_height_bias",
            None,
            1..=64,
        )?,
    })
}

/// `FloatProviders.codec(min, max)`.
fn float(value: &Value, key: &str, range: RangeInclusive<f32>) -> Result<FloatProvider, String> {
    let provider: FloatProvider = field(value, key)?;
    (range.contains(&provider.get_min()) && range.contains(&provider.get_max()))
        .then_some(provider)
        .ok_or_else(|| format!("{key} must be within {range:?}"))
}

/// `Codec.floatRange(min, max)`.
fn real(value: &Value, key: &str, range: RangeInclusive<f32>) -> Result<f32, String> {
    let value: f32 = field(value, key)?;
    range
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("{key} must be within {range:?}"))
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
        proto_chunk::{GenerationCache, ProtoChunk},
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
        "block_blob",
        "huge_brown_mushroom",
        "huge_red_mushroom",
        // Kelp exercises the randomized_int state provider.
        "block_column",
        "speleothem_cluster",
        "large_dripstone",
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
    fn runtime_huge_mushrooms_match_generated_on_mycelium() {
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
        for name in ["huge_brown_mushroom", "huge_red_mushroom"] {
            let json = read_json(&assets.join(format!("{name}.json"))).unwrap();
            let runtime = files
                .configured(&json, &mut |_| Err("unexpected placed holder".into()))
                .unwrap();
            let generated = CONFIGURED_FEATURES
                .get(&ConfiguredId::from_name(&format!("minecraft:{name}")).unwrap())
                .unwrap();
            let grow = |feature: &ConfiguredFeature, seed: u64| {
                let mut chunk = ProtoChunk::new(0, 0, &world_gen);
                chunk.set_block_state(8, 100, 8, pumpkin_data::Block::MYCELIUM.default_state);
                let mut random = RandomGenerator::Worldgen(WorldgenRandom::from_seed(seed));
                let (min_y, height) = (chunk.bottom_y(), chunk.height());
                let placed = feature.generate(
                    &mut chunk,
                    &TestBlockRegistry,
                    min_y,
                    height,
                    PlacedId::from_name("minecraft:amethyst_geode").unwrap(),
                    &mut random,
                    BlockPos::new(8, 101, 8),
                );
                (placed, chunk.flat_block_map)
            };
            for seed in 0..8 {
                let expected = grow(generated, seed);
                assert!(expected.0, "{name} did not grow");
                assert_eq!(grow(&runtime, seed), expected, "{name} seed {seed}");
            }
        }
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

    #[test]
    fn runtime_dripstone_matches_generated_in_caves() {
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
        let WorldGenerator::Noise(generator) = &*world_gen else {
            unreachable!()
        };
        // Air just above a cave floor at x = 8, z = 8 of each chunk, with the untouched blocks.
        let caves: Vec<_> = (0..48)
            .filter_map(|chunk_x| {
                let mut chunk = ProtoChunk::new(chunk_x, 0, &world_gen);
                chunk.step_to_biomes(generator);
                chunk.stage = StagedChunkEnum::StructureReferences;
                chunk.step_to_noise(generator);
                let air = |y| {
                    GenerationCache::get_block_state(
                        &chunk,
                        &pumpkin_util::math::vector3::Vector3::new(chunk_x * 16 + 8, y, 8),
                    )
                    .to_state()
                    .is_air()
                };
                let y = (-56..30).find(|&y| air(y) && !air(y - 1) && air(y + 4))?;
                Some((chunk_x, y, chunk.flat_block_map))
            })
            .collect();
        assert!(caves.len() >= 3, "found {} caves", caves.len());
        for name in ["dripstone_cluster", "large_dripstone"] {
            let json = read_json(&assets.join(format!("{name}.json"))).unwrap();
            let runtime = files
                .configured(&json, &mut |_| Err("unexpected placed holder".into()))
                .unwrap();
            let generated = CONFIGURED_FEATURES
                .get(&ConfiguredId::from_name(&format!("minecraft:{name}")).unwrap())
                .unwrap();
            let mut grown = 0;
            for (chunk_x, y, pristine) in &caves {
                let expected = place(generated, &world_gen, *chunk_x, *y);
                grown += usize::from(&expected.1 != pristine);
                assert_eq!(
                    place(&runtime, &world_gen, *chunk_x, *y),
                    expected,
                    "{name} at chunk {chunk_x} y {y}"
                );
            }
            assert!(grown > 0, "{name} never changed a cave");
        }
    }
}
