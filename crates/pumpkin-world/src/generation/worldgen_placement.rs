use std::collections::BTreeMap;

use pumpkin_data::{
    Block, BlockDirection, BlockState,
    tag::{RegistryKey, get_registry_key_tags},
};
use pumpkin_util::{
    math::{int_provider::IntProvider, position::BlockPos, vector3::Vector3},
    version::JavaMinecraftVersion,
};
use pumpkin_worldgen_model::{
    feature_data::codec_kind, material::YOffsetStruct, noise_settings::BlockStateCodecStruct,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::block::BlockStateCodec;

use super::{
    block_predicate::{
        AllOfBlockPredicate, AnyOfBlockPredicate, BlockPredicate, HasSturdyFacePredicate,
        HeightRangeBlockPredicate, InsideWorldBoundsBlockPredicate, MatchingBlockTagPredicate,
        MatchingBlocksBlockPredicate, MatchingBlocksWrapper, MatchingFluidsBlockPredicate,
        NotBlockPredicate, OffsetBlocksBlockPredicate, ReplaceableBlockPredicate,
        SolidBlockPredicate, UnobstructedBlockPredicate, WouldSurviveBlockPredicate,
    },
    feature::placed_features::{
        BiomePlacementModifier, BlockFilterPlacementModifier, CountOnEveryLayerPlacementModifier,
        CountPlacementModifier, CuboidPlacementModifier, EnvironmentScanPlacementModifier,
        HeightRangePlacementModifier, HeightmapPlacementModifier, NoiseBasedCountPlacementModifier,
        NoiseThresholdCountPlacementModifier, OffsetPlacementModifier, PlacementModifier,
        RandomChancePlacementModifier, RandomOffsetPlacementModifier,
        RarityFilterPlacementModifier, SquarePlacementModifier,
        SurfaceThresholdFilterPlacementModifier, SurfaceWaterDepthFilterPlacementModifier,
    },
    height_provider::{
        BiasedToBottomHeightProvider, HeightProvider, TrapezoidHeightProvider,
        UniformHeightProvider, VeryBiasedToBottomHeightProvider,
    },
    worldgen_overrides::{block_state, resource_id},
};

pub(super) fn read<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

pub(super) fn field<T: DeserializeOwned>(value: &Value, key: &str) -> Result<T, String> {
    read(&value[key]).map_err(|error| format!("{key}: {error}"))
}

/// Reads an optional int field like vanilla's `Codec.INT` reads a Gson number: a fraction is
/// dropped and a value past 32 bits keeps its low 32 bits.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Gson's LazilyParsedNumber.intValue truncates"
)]
pub(super) fn java_int(value: &Value, key: &str) -> Result<Option<i32>, String> {
    let number = match &value[key] {
        Value::Null => return Ok(None),
        Value::Number(number) => number,
        _ => return Err(format!("{key}: expected a number")),
    };
    let wide = match (number.as_i64(), number.as_u64(), number.as_f64()) {
        (Some(int), _, _) => i128::from(int),
        (None, Some(int), _) => i128::from(int),
        (None, None, Some(float)) => float.trunc() as i128,
        (None, None, None) => return Err(format!("{key}: expected a number")),
    };
    Ok(Some(wide as i32))
}

fn anchor(value: &Value) -> Result<pumpkin_util::y_offset::YOffset, String> {
    Ok(read::<YOffsetStruct>(value)?.into_offset())
}

pub(super) fn height(value: &Value) -> Result<HeightProvider, String> {
    if !value
        .as_object()
        .is_some_and(|object| object.contains_key("type"))
    {
        return Ok(HeightProvider::Constant(anchor(value)?));
    }
    let kind = codec_kind(value)?;
    if kind == "constant" {
        return Ok(HeightProvider::Constant(anchor(&value["value"])?));
    }
    let min_inclusive = anchor(&value["min_inclusive"])?;
    let max_inclusive = anchor(&value["max_inclusive"])?;
    match kind {
        "uniform" => Ok(HeightProvider::Uniform(UniformHeightProvider {
            min_inclusive,
            max_inclusive,
        })),
        "trapezoid" => Ok(HeightProvider::Trapezoid(TrapezoidHeightProvider {
            min_inclusive,
            max_inclusive,
            plateau: field(value, "plateau")?,
        })),
        "biased_to_bottom" => Ok(HeightProvider::BiasedToBottom(
            BiasedToBottomHeightProvider {
                min_inclusive,
                max_inclusive,
                inner: field(value, "inner")?,
            },
        )),
        "very_biased_to_bottom" => Ok(HeightProvider::VeryBiasedToBottom(
            VeryBiasedToBottomHeightProvider {
                min_inclusive,
                max_inclusive,
                inner: field(value, "inner")?,
            },
        )),
        _ => Err(format!("unsupported height provider {kind}")),
    }
}

pub(super) fn state(value: &Value) -> Result<&'static BlockState, String> {
    block_state(read(value)?)
}

pub(super) fn state_codec(value: &Value) -> Result<BlockStateCodec, String> {
    let (name, properties) = match read(value)? {
        BlockStateCodecStruct::Name(name) => (name, None),
        BlockStateCodecStruct::Structured { name, properties } => (name, properties),
    };
    Ok(BlockStateCodec {
        name: Block::from_name(&name).ok_or_else(|| format!("unknown block {name}"))?,
        properties: properties.map(|properties| properties.into_iter().collect()),
    })
}

fn offset(value: &Value) -> Result<Option<Vector3<i32>>, String> {
    Ok(field::<Option<[i32; 3]>>(value, "offset")?.map(|[x, y, z]| Vector3::new(x, y, z)))
}

fn matching(value: &Value) -> Result<MatchingBlocksWrapper, String> {
    if let Some(id) = value.as_str() {
        Ok(MatchingBlocksWrapper::Single(id.to_owned()))
    } else {
        Ok(MatchingBlocksWrapper::Multiple(read(value)?))
    }
}

pub(super) type BlockTags = BTreeMap<String, pumpkin_data::tag::Tag>;

pub(super) fn block_tag(value: &str, tags: &BlockTags) -> Result<pumpkin_data::tag::Tag, String> {
    let name = resource_id(value.strip_prefix('#').unwrap_or(value));
    tags.get(name.as_ref())
        .copied()
        .or_else(|| {
            get_registry_key_tags(JavaMinecraftVersion::V_26_3, RegistryKey::Block)
                .and_then(|tags| tags.get(name.as_ref()).map(|tag| **tag))
        })
        .ok_or_else(|| format!("unknown block tag {name}"))
}

pub(super) fn block_set(value: &Value, tags: &BlockTags) -> Result<BlockPredicate, String> {
    let offset = OffsetBlocksBlockPredicate { offset: None };
    if let Some(name) = value.as_str().and_then(|name| name.strip_prefix('#')) {
        return tag(name, offset, tags);
    }
    Ok(BlockPredicate::MatchingBlocks(
        MatchingBlocksBlockPredicate {
            offset,
            blocks: matching(value)?,
        },
    ))
}

fn tag(
    value: &str,
    offset: OffsetBlocksBlockPredicate,
    tags: &BlockTags,
) -> Result<BlockPredicate, String> {
    Ok(BlockPredicate::MatchingBlockTag(
        MatchingBlockTagPredicate {
            offset,
            tag: block_tag(value, tags)?,
        },
    ))
}

pub(super) fn predicate(value: &Value, tags: &BlockTags) -> Result<BlockPredicate, String> {
    if let Some(name) = value.as_str() {
        return match name.strip_prefix('#') {
            Some(id) => tag(id, OffsetBlocksBlockPredicate { offset: None }, tags),
            None if name == "true" => Ok(BlockPredicate::AlwaysTrue),
            None => Err(format!("unsupported block predicate {name}")),
        };
    }
    let offset = OffsetBlocksBlockPredicate {
        offset: offset(value)?,
    };
    let kind = codec_kind(value)?;
    let predicates = || {
        value["predicates"]
            .as_array()
            .ok_or_else(|| "missing predicates".to_owned())?
            .iter()
            .map(|value| predicate(value, tags))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match kind {
        "matching_blocks" => {
            let blocks = &value["blocks"];
            if let Some(name) = blocks.as_str().and_then(|name| name.strip_prefix('#')) {
                return tag(name, offset, tags);
            }
            BlockPredicate::MatchingBlocks(MatchingBlocksBlockPredicate {
                offset,
                blocks: matching(blocks)?,
            })
        }
        "matching_block_tag" => {
            return tag(
                value["tag"].as_str().ok_or("missing block tag")?,
                offset,
                tags,
            );
        }
        "matching_fluids" => BlockPredicate::MatchingFluids(MatchingFluidsBlockPredicate {
            offset,
            fluids: matching(&value["fluids"])?,
        }),
        "has_sturdy_face" => BlockPredicate::HasSturdyFace(HasSturdyFacePredicate {
            offset,
            direction: field(value, "direction")?,
        }),
        "solid" => BlockPredicate::Solid(SolidBlockPredicate { offset }),
        "replaceable" => BlockPredicate::Replaceable(ReplaceableBlockPredicate { offset }),
        "would_survive" => BlockPredicate::WouldSurvive(WouldSurviveBlockPredicate {
            offset,
            state: state_codec(&value["state"])?,
        }),
        "inside_world_bounds" => {
            BlockPredicate::InsideWorldBounds(InsideWorldBoundsBlockPredicate {
                offset: offset.offset.unwrap_or(Vector3::new(0, 0, 0)),
            })
        }
        "height_range" => BlockPredicate::HeightRange(HeightRangeBlockPredicate {
            min_inclusive: anchor(&value["min_inclusive"])?,
            max_inclusive: anchor(&value["max_inclusive"])?,
        }),
        "any_of" => BlockPredicate::AnyOf(AnyOfBlockPredicate {
            predicates: predicates()?,
        }),
        "all_of" => BlockPredicate::AllOf(AllOfBlockPredicate {
            predicates: predicates()?,
        }),
        "not" => BlockPredicate::Not(NotBlockPredicate {
            predicate: Box::new(predicate(&value["predicate"], tags)?),
        }),
        "unobstructed" => BlockPredicate::Unobstructed(UnobstructedBlockPredicate {
            offset: offset.offset,
        }),
        "true" | "always_true" => BlockPredicate::AlwaysTrue,
        _ => return Err(format!("unsupported block predicate {kind}")),
    })
}

pub(super) fn bounded_int(
    value: &Value,
    key: &str,
    range: std::ops::RangeInclusive<i32>,
) -> Result<IntProvider, String> {
    let provider: IntProvider = field(value, key)?;
    let min = provider.get_min();
    let max = provider.get_max();
    if range.contains(&min) && range.contains(&max) && min <= max {
        Ok(provider)
    } else {
        Err(format!("{key} must be within {range:?}"))
    }
}

fn environment_scan(value: &Value, tags: &BlockTags) -> Result<PlacementModifier, String> {
    let direction_of_search = field(value, "direction_of_search")?;
    let max_steps = field(value, "max_steps")?;
    if !matches!(
        direction_of_search,
        BlockDirection::Up | BlockDirection::Down
    ) || !(1..=32).contains(&max_steps)
    {
        return Err("environment scan requires a vertical direction and 1..=32 steps".into());
    }
    Ok(PlacementModifier::EnvironmentScan(
        EnvironmentScanPlacementModifier {
            direction_of_search,
            target_condition: predicate(&value["target_condition"], tags)?,
            allowed_search_condition: value
                .get("allowed_search_condition")
                .map(|value| predicate(value, tags))
                .transpose()?,
            max_steps,
        },
    ))
}

/// Parses modifiers in the order retained by the placed-feature codec.
pub(super) fn modifier(value: &Value, tags: &BlockTags) -> Result<PlacementModifier, String> {
    let kind = codec_kind(value)?;
    Ok(match kind {
        "biome" => PlacementModifier::Biome(BiomePlacementModifier),
        "in_square" => PlacementModifier::InSquare(SquarePlacementModifier),
        "count" => PlacementModifier::Count(CountPlacementModifier {
            count: bounded_int(value, "count", 0..=4096)?,
        }),
        "count_on_every_layer" => {
            PlacementModifier::CountOnEveryLayer(CountOnEveryLayerPlacementModifier {
                count: bounded_int(value, "count", 0..=256)?,
            })
        }
        "heightmap" => PlacementModifier::Heightmap(HeightmapPlacementModifier {
            heightmap: field(value, "heightmap")?,
        }),
        "height_range" => PlacementModifier::HeightRange(HeightRangePlacementModifier {
            height: height(&value["height"])?,
        }),
        "fixed_placement" => PlacementModifier::FixedPlacement(
            field::<Vec<[i32; 3]>>(value, "positions")?
                .into_iter()
                .map(|[x, y, z]| BlockPos::new(x, y, z))
                .collect(),
        ),
        "rarity_filter" => PlacementModifier::RarityFilter(RarityFilterPlacementModifier {
            chance: field::<std::num::NonZeroU32>(value, "chance")?.get(),
        }),
        "random_chance" => PlacementModifier::RandomChance(RandomChancePlacementModifier {
            chance: field(value, "chance")?,
        }),
        "block_predicate_filter" => {
            PlacementModifier::BlockPredicateFilter(BlockFilterPlacementModifier {
                predicate: predicate(&value["predicate"], tags)?,
            })
        }
        "surface_relative_threshold_filter" => PlacementModifier::SurfaceRelativeThresholdFilter(
            SurfaceThresholdFilterPlacementModifier {
                heightmap: field(value, "heightmap")?,
                min_inclusive: field(value, "min_inclusive")?,
                max_inclusive: field(value, "max_inclusive")?,
            },
        ),
        "surface_water_depth_filter" => {
            PlacementModifier::SurfaceWaterDepthFilter(SurfaceWaterDepthFilterPlacementModifier {
                max_water_depth: field(value, "max_water_depth")?,
            })
        }
        "noise_based_count" => {
            PlacementModifier::NoiseBasedCount(NoiseBasedCountPlacementModifier {
                to_count_ratio: field(value, "noise_to_count_ratio")?,
                factor: field(value, "noise_factor")?,
                offset: field::<Option<f64>>(value, "noise_offset")?.unwrap_or_default(),
            })
        }
        "noise_threshold_count" => {
            PlacementModifier::NoiseThresholdCount(NoiseThresholdCountPlacementModifier {
                noise_level: field(value, "noise_level")?,
                below_noise: field(value, "below_noise")?,
                above_noise: field(value, "above_noise")?,
            })
        }
        "environment_scan" => return environment_scan(value, tags),
        "randomly_selected" => {
            let placements = value["placements"]
                .as_array()
                .filter(|list| !list.is_empty())
                .ok_or("randomly_selected placements must be a non-empty list")?;
            PlacementModifier::RandomlySelected(
                placements
                    .iter()
                    .map(|value| modifier(value, tags))
                    .collect::<Result<_, _>>()?,
            )
        }
        "random_offset" => PlacementModifier::RandomOffset(RandomOffsetPlacementModifier {
            xz_spread: bounded_int(value, "xz_spread", -16..=16)?,
            y_spread: bounded_int(value, "y_spread", -16..=16)?,
        }),
        "offset" => PlacementModifier::Offset(OffsetPlacementModifier {
            x: bounded_int(value, "x", -16..=16)?,
            y: bounded_int(value, "y", -16..=16)?,
            z: bounded_int(value, "z", -16..=16)?,
        }),
        "cuboid" => PlacementModifier::Cuboid(CuboidPlacementModifier {
            xz_size: bounded_int(value, "xz_size", 1..=16)?,
            y_size: bounded_int(value, "y_size", 1..=16)?,
            include_edges: field::<Option<bool>>(value, "include_edges")?.unwrap_or(true),
            include_interior: field::<Option<bool>>(value, "include_interior")?.unwrap_or(true),
        }),
        _ => return Err(format!("unsupported placement modifier {kind}")),
    })
}
