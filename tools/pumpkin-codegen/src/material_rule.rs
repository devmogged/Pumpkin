use std::{collections::BTreeMap, fs, path::Path};

use proc_macro2::TokenStream;
use pumpkin_worldgen_model::material::{MaterialRegistry, resolve_rule};
use quote::{format_ident, quote};
use serde_json::Value;

fn material_sources(dir: &Path) -> BTreeMap<String, String> {
    fn collect(dir: &Path, prefix: &str, sources: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(&entry.path(), &format!("{prefix}{name}/"), sources);
            } else if let Some(name) = name.strip_suffix(".json") {
                let path = entry.path();
                let json = fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                sources.insert(format!("{prefix}{name}"), json);
            }
        }
    }
    let mut sources = BTreeMap::new();
    collect(dir, "minecraft:", &mut sources);
    sources
}

fn include_sources(directory: &str, sources: &BTreeMap<String, String>) -> Vec<TokenStream> {
    sources
        .keys()
        .map(|id| {
            let name = id.strip_prefix("minecraft:").unwrap();
            let path = format!(
                "../../../../assets/datapack/data/minecraft/worldgen/{directory}/{name}.json"
            );
            quote!((#id, include_str!(#path)))
        })
        .collect()
}

/// Reads material_rule files and resolves conditions from material_condition folder.
pub fn build() -> TokenStream {
    let worldgen = Path::new("../../assets/datapack/data/minecraft/worldgen");
    let rules = material_sources(&worldgen.join("material_rule"));
    let conditions = material_sources(&worldgen.join("material_condition"));
    let rule_sources = include_sources("material_rule", &rules);
    let condition_sources = include_sources("material_condition", &conditions);

    let top_level_rules = [
        "bedrock_floor",
        "bedrock_roof",
        "end",
        "nether",
        "overworld",
        "overworld_caves",
        "overworld_floating_islands",
    ];

    let mut const_defs = TokenStream::new();

    for rule_name in top_level_rules {
        let resolved = resolve_rule(
            Value::String(format!("minecraft:{rule_name}")),
            |registry, name| {
                let sources = match registry {
                    MaterialRegistry::Rule => &rules,
                    MaterialRegistry::Condition => &conditions,
                };
                let json = sources
                    .get(name)
                    .ok_or_else(|| format!("unknown {} {name}", registry.directory()))?;
                serde_json::from_str(json).map_err(|error| format!("{name}: {error}"))
            },
        )
        .unwrap_or_else(|error| panic!("Failed to resolve {rule_name}: {error}"));

        let const_ident = format_ident!("{}", rule_name.to_uppercase());
        const_defs.extend(quote!(
            pub const #const_ident: MaterialRule = #resolved;
        ));
    }

    quote!(
        use crate::chunk::DoublePerlinNoiseParameters;
        use crate::BlockState;
        use crate::biome::Biome;
        use crate::dimension::Dimension;
        use pumpkin_util::y_offset::YOffset;
        use pumpkin_util::y_offset::Absolute;

        #[derive(Debug, PartialEq)]
        pub struct BlockMaterialRule {
            pub result_state: &'static BlockState,
        }

        #[derive(Debug, PartialEq)]
        pub struct SequenceMaterialRule {
            pub sequence: &'static [MaterialRule],
        }

        #[derive(Debug, PartialEq)]
        pub struct ConditionMaterialRule {
            pub if_true: MaterialCondition,
            pub then_run: &'static MaterialRule,
        }

        #[derive(Debug, PartialEq)]
        pub struct BadLandsMaterialRule;

        #[derive(Debug, PartialEq)]
        pub struct OreVeinMaterialRule {
            pub ore_block: &'static BlockState,
            pub raw_ore_block: &'static BlockState,
            pub filler_block: &'static BlockState,
            pub raw_ore_chance: f32,
            pub min_y: i32,
            pub max_y: i32,
            pub is_iron: bool,
        }

        #[derive(Debug, PartialEq)]
        pub enum MaterialRule {
            Block(BlockMaterialRule),
            Sequence(SequenceMaterialRule),
            Condition(ConditionMaterialRule),
            Badlands(BadLandsMaterialRule),
            OreVein(OreVeinMaterialRule),
        }

        impl MaterialRule {
            #[must_use]
            pub fn from_name(name: &str) -> Option<&'static Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                match name {
                    "overworld" | "amplified" | "large_biomes" => Some(&OVERWORLD),
                    "nether" => Some(&NETHER),
                    "end" => Some(&END),
                    "bedrock_floor" => Some(&BEDROCK_FLOOR),
                    "bedrock_roof" => Some(&BEDROCK_ROOF),
                    "caves" | "overworld_caves" => Some(&OVERWORLD_CAVES),
                    "floating_islands" | "overworld_floating_islands" => Some(&OVERWORLD_FLOATING_ISLANDS),
                    _ => None,
                }
            }

            #[must_use]
            pub fn from_dimension(dimension: &Dimension) -> &'static Self {
                if dimension == &Dimension::OVERWORLD {
                    &OVERWORLD
                } else if dimension == &Dimension::THE_NETHER {
                    &NETHER
                } else {
                    &END
                }
            }
        }

        #[derive(Debug, PartialEq)]
        pub struct BiomeMaterialCondition {
            pub biome_is: &'static [&'static Biome],
        }

        #[derive(Debug, PartialEq)]
        pub struct NoiseThresholdMaterialCondition {
            pub noise: &'static DoublePerlinNoiseParameters,
            pub min_threshold: f64,
            pub max_threshold: f64,
        }

        #[derive(Debug, PartialEq)]
        pub struct VerticalGradientMaterialCondition {
            pub random_lo: u64,
            pub random_hi: u64,
            pub random_hash: i32,
            pub true_at_and_below: YOffset,
            pub false_at_and_above: YOffset,
        }

        #[derive(Debug, PartialEq)]
        pub struct AboveYMaterialCondition {
            pub anchor: YOffset,
            pub surface_depth_multiplier: i32,
            pub add_stone_depth: bool,
        }

        #[derive(Debug, PartialEq)]
        pub struct WaterMaterialCondition {
            pub offset: i32,
            pub surface_depth_multiplier: i32,
            pub add_stone_depth: bool,
        }

        #[derive(Debug, PartialEq)]
        pub struct HoleMaterialCondition;

        #[derive(Debug, PartialEq)]
        pub struct NotMaterialCondition {
            pub invert: &'static MaterialCondition,
        }

        #[derive(Debug, PartialEq)]
        pub struct SurfaceMaterialCondition;

        #[derive(Debug, PartialEq)]
        pub struct StoneDepthMaterialCondition {
            pub offset: i32,
            pub add_surface_depth: bool,
            pub secondary_depth_range: i32,
            pub surface_type: pumpkin_util::math::vertical_surface_type::VerticalSurfaceType,
        }

        #[derive(Debug, PartialEq)]
        pub enum MaterialCondition {
            Biome(BiomeMaterialCondition),
            NoiseThreshold(NoiseThresholdMaterialCondition),
            VerticalGradient(VerticalGradientMaterialCondition),
            YAbove(AboveYMaterialCondition),
            Water(WaterMaterialCondition),
            Temperature,
            Steep,
            Not(NotMaterialCondition),
            Hole(HoleMaterialCondition),
            AbovePreliminarySurface(SurfaceMaterialCondition),
            StoneDepth(StoneDepthMaterialCondition),
            NoiseThreshold3d(NoiseThresholdMaterialCondition),
        }

        pub const VANILLA_MATERIAL_RULES: &[(&str, &str)] = &[#(#rule_sources),*];
        pub const VANILLA_MATERIAL_CONDITIONS: &[(&str, &str)] = &[#(#condition_sources),*];

        #const_defs
    )
}
