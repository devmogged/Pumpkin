use std::ops::RangeInclusive;

use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::math::int_provider::UniformIntProvider;
use pumpkin_worldgen_model::feature_data::codec_kind;
use serde_json::Value;

use super::{
    block_state_provider::BlockStateProvider,
    feature::{
        features::tree::{
            TreeFeature,
            decorator::{
                TreeDecorator, alter_ground::AlterGroundTreeDecorator,
                attached_to_leaves::AttachedToLeavesTreeDecorator,
                attached_to_logs::AttachedToLogsTreeDecorator, beehive::BeehiveTreeDecorator,
                cocoa::CocoaTreeDecorator, creaking_heart::CreakingHeartTreeDecorator,
                leave_vine::LeavesVineTreeDecorator, pale_moss::PaleMossTreeDecorator,
                place_on_ground::PlaceOnGroundTreeDecorator, trunk_vine::TrunkVineTreeDecorator,
            },
            foliage::{
                FoliagePlacer, FoliageType, acacia::AcaciaFoliagePlacer, blob::BlobFoliagePlacer,
                bush::BushFoliagePlacer, cherry::CherryFoliagePlacer,
                dark_oak::DarkOakFoliagePlacer, fancy::LargeOakFoliagePlacer,
                jungle::JungleFoliagePlacer, mega_pine::MegaPineFoliagePlacer,
                pine::PineFoliagePlacer, poplar::PoplarFoliagePlacer,
                random_spread::RandomSpreadFoliagePlacer, spruce::SpruceFoliagePlacer,
            },
            root::{
                RootPlacer,
                mangrove::{AboveRootPlacement, MangroveRootPlacement, MangroveRootPlacer},
            },
            trunk::{
                TrunkPlacer, TrunkType, bending::BendingTrunkPlacer, cherry::CherryTrunkPlacer,
                dark_oak::DarkOakTrunkPlacer, fancy::FancyTrunkPlacer, forking::ForkingTrunkPlacer,
                giant::GiantTrunkPlacer, mega_jungle::MegaJungleTrunkPlacer,
                poplar::PoplarTrunkPlacer, straight::StraightTrunkPlacer,
                upwards_branching::UpwardsBranchingTrunkPlacer,
            },
        },
        size::{FeatureSize, FeatureSizeType, ThreeLayersFeatureSize, TwoLayersFeatureSize},
    },
    worldgen_placement::{BlockTags, block_tag, bounded_int, field, read},
};

pub(super) fn compile(
    config: &Value,
    tags: &BlockTags,
    provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
) -> Result<TreeFeature, String> {
    let decorators = array(config, "decorators")?
        .iter()
        .map(|value| decorator(value, provider))
        .collect::<Result<_, _>>()?;
    Ok(TreeFeature {
        trunk_provider: provider(&config["trunk_provider"])?,
        trunk_placer: trunk(&config["trunk_placer"], tags)?,
        foliage_provider: provider(&config["foliage_provider"])?,
        foliage_placer: foliage(&config["foliage_placer"])?,
        minimum_size: size(&config["minimum_size"])?,
        ignore_vines: field::<Option<bool>>(config, "ignore_vines")?.unwrap_or(false),
        decorators,
        below_trunk_provider: provider(&config["below_trunk_provider"])?,
        root_placer: config
            .get("root_placer")
            .map(|value| root(value, tags, provider))
            .transpose()?,
    })
}

fn trunk(v: &Value, tags: &BlockTags) -> Result<TrunkPlacer, String> {
    let r#type = match codec_kind(v)? {
        "straight_trunk_placer" => TrunkType::Straight(StraightTrunkPlacer),
        "forking_trunk_placer" => TrunkType::Forking(ForkingTrunkPlacer),
        "giant_trunk_placer" => TrunkType::Giant(GiantTrunkPlacer),
        "mega_jungle_trunk_placer" => TrunkType::MegaJungle(MegaJungleTrunkPlacer),
        "dark_oak_trunk_placer" => TrunkType::DarkOak(DarkOakTrunkPlacer),
        "fancy_trunk_placer" => TrunkType::Fancy(FancyTrunkPlacer),
        "bending_trunk_placer" => TrunkType::Bending(BendingTrunkPlacer {
            min_height_for_leaves: integer(v, "min_height_for_leaves", Some(1), 1..=i32::MAX)?
                as u32,
            bend_length: bounded_int(v, "bend_length", 1..=64)?,
        }),
        "upwards_branching_trunk_placer" => {
            TrunkType::UpwardsBranching(UpwardsBranchingTrunkPlacer {
                extra_branch_steps: bounded_int(v, "extra_branch_steps", 1..=i32::MAX)?,
                place_branch_per_log_probability: probability(
                    v,
                    "place_branch_per_log_probability",
                )?,
                extra_branch_length: bounded_int(v, "extra_branch_length", 0..=i32::MAX)?,
                can_grow_through: blocks(&v["can_grow_through"], tags)?,
            })
        }
        "cherry_trunk_placer" => {
            let start_offset_from_top: UniformIntProvider =
                field(v, "branch_start_offset_from_top")?;
            let min = start_offset_from_top.min_inclusive;
            let max = start_offset_from_top.max_inclusive;
            if !(-16..=0).contains(&min) || !(-16..=0).contains(&max) || min >= max {
                return Err(
                    "cherry branch start offsets need at least two values in -16..=0".into(),
                );
            }
            TrunkType::Cherry(CherryTrunkPlacer {
                count: bounded_int(v, "branch_count", 1..=3)?,
                horizontal_length: bounded_int(v, "branch_horizontal_length", 2..=16)?,
                start_offset_from_top,
                end_offset_from_top: bounded_int(v, "branch_end_offset_from_top", -16..=16)?,
            })
        }
        "poplar_trunk_placer" => TrunkType::Poplar(PoplarTrunkPlacer {
            trunk_height_above_branches: bounded_int(v, "trunk_height_above_branches", 0..=8)?,
            branch_amount: bounded_int(v, "branch_amount", 1..=4)?,
        }),
        kind => return Err(format!("unsupported trunk placer {kind}")),
    };
    Ok(TrunkPlacer {
        base_height: integer(v, "base_height", None, 0..=32)? as u8,
        height_rand_a: integer(v, "height_rand_a", None, 0..=24)? as u8,
        height_rand_b: integer(v, "height_rand_b", None, 0..=24)? as u8,
        r#type,
    })
}

fn foliage(v: &Value) -> Result<FoliagePlacer, String> {
    let r#type = match codec_kind(v)? {
        "blob_foliage_placer" => FoliageType::Blob(BlobFoliagePlacer {
            height: integer(v, "height", None, 0..=16)?,
        }),
        "spruce_foliage_placer" => FoliageType::Spruce(SpruceFoliagePlacer {
            trunk_height: bounded_int(v, "trunk_height", 0..=24)?,
        }),
        "pine_foliage_placer" => FoliageType::Pine(PineFoliagePlacer {
            height: bounded_int(v, "height", 0..=24)?,
        }),
        "acacia_foliage_placer" => FoliageType::Acacia(AcaciaFoliagePlacer),
        "bush_foliage_placer" => FoliageType::Bush(BushFoliagePlacer {
            height: integer(v, "height", None, 0..=16)?,
        }),
        "fancy_foliage_placer" => FoliageType::Fancy(LargeOakFoliagePlacer {
            height: integer(v, "height", None, 0..=16)?,
        }),
        "jungle_foliage_placer" => FoliageType::Jungle(JungleFoliagePlacer {
            height: integer(v, "height", None, 0..=16)?,
        }),
        "mega_pine_foliage_placer" => FoliageType::MegaPine(MegaPineFoliagePlacer {
            crown_height: bounded_int(v, "crown_height", 0..=24)?,
        }),
        "dark_oak_foliage_placer" => FoliageType::DarkOak(DarkOakFoliagePlacer),
        "random_spread_foliage_placer" => FoliageType::RandomSpread(RandomSpreadFoliagePlacer {
            foliage_height: bounded_int(v, "foliage_height", 1..=512)?,
            leaf_placement_attempts: integer(v, "leaf_placement_attempts", None, 0..=256)?,
        }),
        "cherry_foliage_placer" => FoliageType::Cherry(CherryFoliagePlacer {
            height: bounded_int(v, "height", 4..=16)?,
            wide_bottom_layer_hole_chance: probability(v, "wide_bottom_layer_hole_chance")?,
            corner_hole_chance: probability(v, "corner_hole_chance")?,
            hanging_leaves_chance: probability(v, "hanging_leaves_chance")?,
            hanging_leaves_extension_chance: probability(v, "hanging_leaves_extension_chance")?,
        }),
        "poplar_foliage_placer" => FoliageType::Poplar(PoplarFoliagePlacer {
            height: bounded_int(v, "height", 5..=16)?,
            side_hole_chance: probability(v, "side_hole_chance")?,
        }),
        kind => return Err(format!("unsupported foliage placer {kind}")),
    };
    Ok(FoliagePlacer {
        radius: bounded_int(v, "radius", 0..=16)?,
        offset: bounded_int(v, "offset", 0..=16)?,
        r#type,
    })
}

fn size(v: &Value) -> Result<FeatureSize, String> {
    let lower_size = integer(v, "lower_size", Some(0), 0..=16)? as u8;
    let upper_size = integer(v, "upper_size", Some(1), 0..=16)? as u8;
    let r#type = match codec_kind(v)? {
        "two_layers_feature_size" => FeatureSizeType::TwoLayersFeatureSize(TwoLayersFeatureSize {
            limit: integer(v, "limit", Some(1), 0..=81)? as u8,
            lower_size,
            upper_size,
        }),
        "three_layers_feature_size" => {
            FeatureSizeType::ThreeLayersFeatureSize(ThreeLayersFeatureSize {
                limit: integer(v, "limit", Some(1), 0..=80)? as u8,
                upper_limit: integer(v, "upper_limit", Some(1), 0..=80)? as u8,
                lower_size,
                middle_size: integer(v, "middle_size", Some(1), 0..=16)? as u8,
                upper_size,
            })
        }
        kind => return Err(format!("unsupported feature size {kind}")),
    };
    let min_clipped_height = v
        .get("min_clipped_height")
        .map(|_| integer(v, "min_clipped_height", None, 0..=80).map(|n| n as u8))
        .transpose()?;
    Ok(FeatureSize {
        min_clipped_height,
        r#type,
    })
}

fn decorator(
    v: &Value,
    provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
) -> Result<TreeDecorator, String> {
    Ok(match codec_kind(v)? {
        "trunk_vine" => TreeDecorator::TrunkVine(TrunkVineTreeDecorator),
        "leave_vine" => TreeDecorator::LeaveVine(LeavesVineTreeDecorator {
            probability: probability(v, "probability")?,
        }),
        "cocoa" => TreeDecorator::Cocoa(CocoaTreeDecorator {
            probability: probability(v, "probability")?,
        }),
        "beehive" => TreeDecorator::Beehive(BeehiveTreeDecorator {
            probability: probability(v, "probability")?,
        }),
        "creaking_heart" => TreeDecorator::CreakingHeart(CreakingHeartTreeDecorator {
            probability: probability(v, "probability")?,
        }),
        "alter_ground" => TreeDecorator::AlterGround(AlterGroundTreeDecorator {
            provider: provider(&v["provider"])?,
        }),
        "attached_to_logs" => TreeDecorator::AttachedToLogs(AttachedToLogsTreeDecorator {
            probability: probability(v, "probability")?,
            block_provider: provider(&v["block_provider"])?,
            directions: directions(v)?,
        }),
        "attached_to_leaves" => TreeDecorator::AttachedToLeaves(AttachedToLeavesTreeDecorator {
            probability: probability(v, "probability")?,
            exclusion_radius_xz: integer(v, "exclusion_radius_xz", None, 0..=16)?,
            exclusion_radius_y: integer(v, "exclusion_radius_y", None, 0..=16)?,
            block_provider: provider(&v["block_provider"])?,
            required_empty_blocks: integer(v, "required_empty_blocks", None, 1..=16)?,
            directions: directions(v)?,
        }),
        "place_on_ground" => TreeDecorator::PlaceOnGround(PlaceOnGroundTreeDecorator {
            tries: integer(v, "tries", Some(128), 1..=i32::MAX)?,
            radius: integer(v, "radius", Some(2), 0..=i32::MAX)?,
            height: integer(v, "height", Some(1), 0..=i32::MAX)?,
            block_state_provider: provider(&v["block_state_provider"])?,
        }),
        "pale_moss" => TreeDecorator::PaleMoss(PaleMossTreeDecorator {
            leaves_probability: probability(v, "leaves_probability")?,
            trunk_probability: probability(v, "trunk_probability")?,
            ground_probability: probability(v, "ground_probability")?,
        }),
        kind => return Err(format!("unsupported tree decorator {kind}")),
    })
}

fn root(
    v: &Value,
    tags: &BlockTags,
    provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
) -> Result<RootPlacer, String> {
    let kind = codec_kind(v)?;
    if kind != "mangrove_root_placer" {
        return Err(format!("unsupported root placer {kind}"));
    }
    let above_root_placement = v
        .get("above_root_placement")
        .map(|above| {
            Ok::<_, String>(AboveRootPlacement {
                above_root_provider: provider(&above["above_root_provider"])?,
                above_root_placement_chance: probability(above, "above_root_placement_chance")?,
            })
        })
        .transpose()?;
    let placement = &v["mangrove_root_placement"];
    Ok(RootPlacer::Mangrove(MangroveRootPlacer {
        trunk_offset_y: field(v, "trunk_offset_y")?,
        root_provider: provider(&v["root_provider"])?,
        above_root_placement,
        mangrove_root_placement: MangroveRootPlacement {
            can_grow_through: blocks(&placement["can_grow_through"], tags)?,
            muddy_roots_in: blocks(&placement["muddy_roots_in"], tags)?,
            muddy_roots_provider: provider(&placement["muddy_roots_provider"])?,
            max_root_width: integer(placement, "max_root_width", None, 1..=12)?,
            max_root_length: integer(placement, "max_root_length", None, 1..=64)?,
            random_skew_chance: probability(placement, "random_skew_chance")?,
        },
    }))
}

fn directions(v: &Value) -> Result<Vec<BlockDirection>, String> {
    let directions: Vec<BlockDirection> = field(v, "directions")?;
    if directions.is_empty() {
        return Err("tree decorator directions must not be empty".into());
    }
    Ok(directions)
}

fn blocks(value: &Value, tags: &BlockTags) -> Result<&'static [u16], String> {
    if let Some(tag) = value.as_str().and_then(|name| name.strip_prefix('#')) {
        return Ok(block_tag(tag, tags)?.1);
    }
    let names: Vec<String> = match value.as_str() {
        Some(name) => vec![name.to_owned()],
        None => read(value)?,
    };
    let ids = names
        .iter()
        .map(|name| {
            Block::from_name(name)
                .map(|block| block.id.as_u16())
                .ok_or_else(|| format!("unknown block {name}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Box::leak(ids.into_boxed_slice()))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing or invalid {key}"))
}

fn integer(
    value: &Value,
    key: &str,
    default: Option<i32>,
    range: RangeInclusive<i32>,
) -> Result<i32, String> {
    let value = field::<Option<i32>>(value, key)?
        .or(default)
        .ok_or_else(|| format!("missing {key}"))?;
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(format!("{key} must be within {range:?}"))
    }
}

fn probability(v: &Value, key: &str) -> Result<f32, String> {
    let p = field(v, key)?;
    if (0.0..=1.0).contains(&p) {
        Ok(p)
    } else {
        Err(format!("{key} must be within 0..=1"))
    }
}
