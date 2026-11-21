use pumpkin_util::math::vertical_surface_type::VerticalSurfaceType;
use pumpkin_worldgen_model::feature_data::probability;
use serde_json::Value;

use super::{
    block_state_provider::BlockStateProvider,
    feature::{
        configured_features::ConfiguredFeature,
        features::{
            disk::DiskFeature,
            random_boolean_selector::RandomBooleanFeature,
            random_selector::{RandomFeature, RandomFeatureEntry},
            sequence::SequenceFeature,
            simple_random_selector::SimpleRandomFeature,
            vegetation_patch::VegetationPatchFeature,
            waterlogged_vegetation_patch::WaterloggedVegetationPatchFeature,
            weighted_random_selector::{WeightedRandomFeature, WeightedRandomFeatureEntry},
        },
        placed_features::PlacedFeatureWrapper,
    },
    worldgen_placement::{BlockTags, block_set, bounded_int, field, predicate},
};

pub(super) fn compile(
    kind: &str,
    config: &Value,
    tags: &BlockTags,
    state_provider: &impl Fn(&Value) -> Result<BlockStateProvider, String>,
    placed: &mut impl FnMut(&Value) -> Result<PlacedFeatureWrapper, String>,
) -> Result<ConfiguredFeature, String> {
    Ok(match kind {
        "random_selector" => ConfiguredFeature::RandomSelector(RandomFeature {
            features: entries(config)?
                .iter()
                .map(|entry| {
                    Ok(RandomFeatureEntry {
                        feature: placed(&entry["feature"])?,
                        chance: probability(entry, "chance")?,
                    })
                })
                .collect::<Result<_, String>>()?,
            default: Box::new(placed(&config["default"])?),
        }),
        "random_boolean_selector" => {
            ConfiguredFeature::RandomBooleanSelector(RandomBooleanFeature {
                feature_true: Box::new(placed(&config["feature_true"])?),
                feature_false: Box::new(placed(&config["feature_false"])?),
            })
        }
        "weighted_random_selector" => {
            let mut total_weight = 0i32;
            let features = entries(config)?
                .iter()
                .map(|entry| {
                    let weight = field::<i32>(entry, "weight")?;
                    total_weight = total_weight
                        .checked_add(weight)
                        .filter(|_| weight >= 0)
                        .ok_or("invalid selector weight")?;
                    Ok(WeightedRandomFeatureEntry {
                        feature: placed(&entry["data"])?,
                        weight,
                    })
                })
                .collect::<Result<_, String>>()?;
            ConfiguredFeature::WeightedRandomSelector(WeightedRandomFeature {
                features,
                total_weight,
            })
        }
        "simple_random_selector" | "sequence" => {
            let features = entries(config)?
                .iter()
                .map(placed)
                .collect::<Result<Vec<_>, _>>()?;
            if kind == "sequence" {
                ConfiguredFeature::Sequence(SequenceFeature { features })
            } else {
                if features.is_empty() {
                    return Err("simple random selector cannot be empty".into());
                }
                ConfiguredFeature::SimpleRandomSelector(SimpleRandomFeature { features })
            }
        }
        "disk" => {
            let half_height = field(config, "half_height")?;
            if !(0..=4).contains(&half_height) {
                return Err("disk half_height must be within 0..=4".into());
            }
            ConfiguredFeature::Disk(DiskFeature {
                state_provider: state_provider(&config["state_provider"])?,
                target: predicate(&config["target"], tags)?,
                radius: bounded_int(config, "radius", 0..=8)?,
                half_height,
            })
        }
        "vegetation_patch" | "waterlogged_vegetation_patch" => {
            let surface = match config["surface"].as_str() {
                Some("floor") => VerticalSurfaceType::Floor,
                Some("ceiling") => VerticalSurfaceType::Ceiling,
                _ => return Err("vegetation patch surface must be floor or ceiling".into()),
            };
            let vertical_range = field(config, "vertical_range")?;
            if !(1..=256).contains(&vertical_range) {
                return Err("vegetation patch vertical_range must be within 1..=256".into());
            }
            let feature = VegetationPatchFeature {
                replaceable: block_set(&config["replaceable"], tags)?,
                ground_state: state_provider(&config["ground_state"])?,
                vegetation_feature: Box::new(placed(&config["vegetation_feature"])?),
                surface,
                depth: bounded_int(config, "depth", 1..=128)?,
                extra_bottom_block_chance: probability(config, "extra_bottom_block_chance")?,
                vertical_range,
                vegetation_chance: probability(config, "vegetation_chance")?,
                xz_radius: field(config, "xz_radius")?,
                extra_edge_column_chance: probability(config, "extra_edge_column_chance")?,
            };
            if kind == "vegetation_patch" {
                ConfiguredFeature::VegetationPatch(feature)
            } else {
                ConfiguredFeature::WaterloggedVegetationPatch(WaterloggedVegetationPatchFeature {
                    base: feature,
                })
            }
        }
        _ => return Err(format!("unsupported configured feature type {kind}")),
    })
}

fn entries(config: &Value) -> Result<&[Value], String> {
    config["features"]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| "features must be a list of holders".to_owned())
}
