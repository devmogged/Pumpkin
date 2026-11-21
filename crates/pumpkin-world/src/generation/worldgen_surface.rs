use std::collections::BTreeMap;

use pumpkin_data::{
    biome::Biome,
    chunk::DoublePerlinNoiseParameters,
    material_rule::{
        AboveYMaterialCondition, BadLandsMaterialRule, BiomeMaterialCondition, BlockMaterialRule,
        ConditionMaterialRule, HoleMaterialCondition, MaterialCondition, MaterialRule,
        NoiseThresholdMaterialCondition, NotMaterialCondition, OreVeinMaterialRule,
        SequenceMaterialRule, StoneDepthMaterialCondition, SurfaceMaterialCondition,
        VerticalGradientMaterialCondition, WaterMaterialCondition,
    },
    tag::{RegistryKey, get_tag_ids},
};
use pumpkin_util::math::vertical_surface_type::VerticalSurfaceType;
use pumpkin_worldgen_model::material::{
    MaterialConditionStruct, MaterialRuleStruct, Surface, gradient_key,
};

use super::worldgen_overrides::{block_state, resource_id};

pub(super) fn compile(
    source: MaterialRuleStruct,
    noise: &impl Fn(&str) -> Result<&'static DoublePerlinNoiseParameters, String>,
    tags: &BTreeMap<String, Vec<u16>>,
) -> Result<MaterialRule, String> {
    use MaterialRuleStruct as Rule;
    Ok(match source {
        Rule::Block { result_state } => MaterialRule::Block(BlockMaterialRule {
            result_state: block_state(result_state)?,
        }),
        Rule::Sequence { sequence } => {
            let rules = sequence
                .into_iter()
                .map(|rule| compile(rule, noise, tags))
                .collect::<Result<Vec<_>, _>>()?;
            MaterialRule::Sequence(SequenceMaterialRule {
                sequence: Box::leak(rules.into_boxed_slice()),
            })
        }
        Rule::Condition { if_true, then_run } => MaterialRule::Condition(ConditionMaterialRule {
            if_true: compile_condition(if_true, noise, tags)?,
            then_run: Box::leak(Box::new(compile(*then_run, noise, tags)?)),
        }),
        Rule::Badlands => MaterialRule::Badlands(BadLandsMaterialRule),
        Rule::OreVein {
            ore_block,
            raw_ore_block,
            filler_block,
            raw_ore_chance,
            min_y,
            max_y,
            is_iron,
        } => MaterialRule::OreVein(OreVeinMaterialRule {
            ore_block: block_state(ore_block)?,
            raw_ore_block: block_state(raw_ore_block)?,
            filler_block: block_state(filler_block)?,
            raw_ore_chance,
            min_y,
            max_y,
            is_iron,
        }),
    })
}

fn compile_condition(
    source: MaterialConditionStruct,
    noise_parameters: &impl Fn(&str) -> Result<&'static DoublePerlinNoiseParameters, String>,
    tags: &BTreeMap<String, Vec<u16>>,
) -> Result<MaterialCondition, String> {
    use MaterialConditionStruct as Condition;
    Ok(match source {
        Condition::Biome { biome_is } => {
            let mut biomes = Vec::new();
            for id in biome_is {
                if let Some(tag) = id.strip_prefix('#') {
                    let name = resource_id(tag);
                    let ids = tags
                        .get(name.as_ref())
                        .map(Vec::as_slice)
                        .or_else(|| get_tag_ids(RegistryKey::WorldgenBiome, &name))
                        .ok_or_else(|| format!("unknown biome tag {name}"))?;
                    biomes.extend(ids.iter().filter_map(|&id| Biome::from_id(id as u8)));
                } else {
                    biomes
                        .push(Biome::from_name(&id).ok_or_else(|| format!("unknown biome {id}"))?);
                }
            }
            MaterialCondition::Biome(BiomeMaterialCondition {
                biome_is: Box::leak(biomes.into_boxed_slice()),
            })
        }
        Condition::NoiseThreshold {
            noise,
            min_threshold,
            max_threshold,
            is_3d,
        } => {
            let threshold = NoiseThresholdMaterialCondition {
                noise: noise_parameters(&noise)?,
                min_threshold,
                max_threshold,
            };
            if is_3d {
                MaterialCondition::NoiseThreshold3d(threshold)
            } else {
                MaterialCondition::NoiseThreshold(threshold)
            }
        }
        Condition::VerticalGradient {
            random_name,
            true_at_and_below,
            false_at_and_above,
        } => {
            let (random_lo, random_hi, random_hash) = gradient_key(&random_name);
            MaterialCondition::VerticalGradient(VerticalGradientMaterialCondition {
                random_lo,
                random_hi,
                random_hash,
                true_at_and_below: true_at_and_below.into_offset(),
                false_at_and_above: false_at_and_above.into_offset(),
            })
        }
        Condition::YAbove {
            anchor: offset,
            surface_depth_multiplier,
            add_stone_depth,
        } => MaterialCondition::YAbove(AboveYMaterialCondition {
            anchor: offset.into_offset(),
            surface_depth_multiplier,
            add_stone_depth,
        }),
        Condition::Water {
            offset,
            surface_depth_multiplier,
            add_stone_depth,
        } => MaterialCondition::Water(WaterMaterialCondition {
            offset,
            surface_depth_multiplier,
            add_stone_depth,
        }),
        Condition::Temperature => MaterialCondition::Temperature,
        Condition::Steep => MaterialCondition::Steep,
        Condition::Not { invert } => MaterialCondition::Not(NotMaterialCondition {
            invert: Box::leak(Box::new(compile_condition(
                *invert,
                noise_parameters,
                tags,
            )?)),
        }),
        Condition::Hole => MaterialCondition::Hole(HoleMaterialCondition),
        Condition::AbovePreliminarySurface => {
            MaterialCondition::AbovePreliminarySurface(SurfaceMaterialCondition)
        }
        Condition::StoneDepth {
            offset,
            add_surface_depth,
            secondary_depth_range,
            surface_type,
        } => MaterialCondition::StoneDepth(StoneDepthMaterialCondition {
            offset,
            add_surface_depth,
            secondary_depth_range,
            surface_type: match surface_type {
                Surface::Ceiling => VerticalSurfaceType::Ceiling,
                Surface::Floor => VerticalSurfaceType::Floor,
            },
        }),
    })
}
