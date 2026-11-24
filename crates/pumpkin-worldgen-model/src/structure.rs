//! Structure and structure set JSON, shared by codegen and the runtime datapack loader.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Deserialized structure set containing placement rules and weighted structure entries.
#[derive(Deserialize)]
pub struct StructureSetStruct {
    /// Placement algorithm and parameters for distributing this structure set.
    pub placement: StructurePlacementStruct,
    /// Weighted list of structures that belong to this set.
    pub structures: Vec<WeightedEntryStruct>,
}

/// Deserialized weighted structure entry within a structure set.
#[derive(Deserialize, Clone)]
pub struct WeightedEntryStruct {
    /// Registry key of the structure (e.g., `"minecraft:village_plains"`).
    pub structure: String,
    /// Relative weight controlling how often this structure is chosen.
    pub weight: u32,
}

/// Reads an integer like vanilla's `ExtraCodecs.NON_NEGATIVE_INT` through Gson, whose
/// `intValue()` keeps the low 32 bits of a larger number.
fn java_non_negative_int<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    let value = i64::deserialize(deserializer)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Gson's LazilyParsedNumber.intValue truncates a long"
    )]
    let int = value as i32;
    u32::try_from(int)
        .map_err(|_| serde::de::Error::custom(format!("value must be non-negative: {value}")))
}

/// Deserialized placement configuration for a structure set.
#[derive(Deserialize)]
pub struct StructurePlacementStruct {
    /// Optional frequency-reduction method name applied before placement.
    pub frequency_reduction_method: Option<String>,
    /// Optional probability (0–1) that a candidate chunk actually spawns the structure.
    pub frequency: Option<f32>,
    /// Per-structure salt mixed into the placement RNG seed.
    #[serde(deserialize_with = "java_non_negative_int")]
    pub salt: u32,
    /// Optional exclusion zone to prevent this structure from generating near others.
    pub exclusion_zone: Option<ExclusionZoneStruct>,
    /// The specific placement algorithm and its parameters.
    #[serde(flatten)]
    pub r#type: StructurePlacementTypeStruct,
}

/// Deserialized exclusion zone configuration.
#[derive(Deserialize)]
pub struct ExclusionZoneStruct {
    pub other_set: String,
    pub chunk_count: i32,
}

/// Deserialized placement algorithm for a structure set.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum StructurePlacementTypeStruct {
    /// Places structures at pseudo-random positions within regularly spaced grid regions.
    #[serde(rename = "minecraft:random_spread")]
    RandomSpread {
        /// Region size (spacing between potential structure positions).
        spacing: i32,
        /// Minimum separation between two structures in the same region.
        separation: i32,
        /// Optional distribution type (`"linear"` or `"triangular"`).
        spread_type: Option<String>,
    },
    /// Places structures in concentric rings around the world origin.
    #[serde(rename = "minecraft:concentric_rings")]
    ConcentricRings {
        /// Angular spread between ring placements.
        spread: i32,
        /// Distance increment between successive rings.
        distance: i32,
        /// Total number of structures placed across all rings.
        count: i32,
        /// Biome tag that ring placements must be located within.
        preferred_biomes: String,
    },
}

/// Deserialized structure pool alias binding.
#[derive(Deserialize, Clone)]
#[serde(tag = "type")]
pub enum PoolAliasBindingStruct {
    #[serde(rename = "minecraft:direct")]
    Direct { alias: String, target: String },
    #[serde(rename = "minecraft:random")]
    Random {
        alias: String,
        targets: Vec<WeightedTargetStruct>,
    },
    #[serde(rename = "minecraft:random_group")]
    RandomGroup { groups: Vec<WeightedGroupStruct> },
}

#[derive(Deserialize, Clone)]
pub struct WeightedTargetStruct {
    pub data: String,
    pub weight: u32,
}

#[derive(Deserialize, Clone)]
pub struct WeightedGroupStruct {
    pub data: Vec<PoolAliasBindingStruct>,
    pub weight: u32,
}

#[derive(Deserialize, Clone)]
pub struct SpawnOverrideStruct {
    pub bounding_box: String,
    pub spawns: Vec<SpawnEntryStruct>,
}

#[derive(Deserialize, Clone)]
pub struct SpawnEntryStruct {
    #[serde(rename = "type")]
    pub entity_type: String,
    #[serde(default = "default_spawn_count", rename = "minCount")]
    pub min_count: u32,
    #[serde(default = "default_spawn_count", rename = "maxCount")]
    pub max_count: u32,
    pub weight: u32,
}

const fn default_spawn_count() -> u32 {
    1
}

/// Deserialized structure entry specifying its valid biomes and generation step.
#[derive(Deserialize, Clone)]
pub struct StructureStruct {
    /// Biomes this structure can start in: a tag, one biome or a list.
    pub biomes: HolderSetJson,
    /// Generation step during which this structure is placed.
    pub step: String,
    /// Optional jigsaw start pool.
    pub start_pool: Option<String>,
    /// Optional named jigsaw in the start pool that must be placed at the start position.
    pub start_jigsaw_name: Option<String>,
    /// Optional jigsaw size (depth).
    pub size: Option<i32>,
    /// Optional terrain adaptation (bearding).
    pub terrain_adaptation: Option<String>,
    /// Optional jigsaw start height.
    pub start_height: Option<serde_json::Value>,
    /// Optional height provider (e.g. for nether fossils).
    pub height: Option<serde_json::Value>,
    /// Optional heightmap to project the start to.
    pub project_start_to_heightmap: Option<String>,
    /// Optional max distance from center.
    pub max_distance_from_center: Option<MaxDistanceJson>,
    /// Optional liquid settings.
    pub liquid_settings: Option<String>,
    /// Optional dimension padding.
    pub dimension_padding: Option<i32>,
    /// Critical for villages to appropriately truncate long village streets.
    pub use_expansion_hack: Option<bool>,
    /// Optional pool alias bindings (e.g. for Trial Chambers).
    #[serde(default)]
    pub pool_aliases: Vec<PoolAliasBindingStruct>,
    /// Spawn overrides (e.g. for Ocean Monuments).
    #[serde(default)]
    pub spawn_overrides: BTreeMap<String, SpawnOverrideStruct>,
    /// Optional beached flag for shipwrecks.
    pub is_beached: Option<bool>,
    /// Optional mineshaft variant ("normal" or "mesa").
    pub mineshaft_type: Option<String>,
    /// Optional ocean ruin temperature ("cold" or "warm").
    pub biome_temp: Option<String>,
    /// Optional cluster probability for ocean ruins.
    pub cluster_probability: Option<f32>,
    /// Optional large probability for ocean ruins.
    pub large_probability: Option<f32>,
    /// Defines the generation behavior (e.g. "minecraft:jigsaw").
    #[serde(rename = "type")]
    pub structure_type: String,
}

#[cfg(feature = "codegen")]
pub use tokens::structure_key_to_token;

#[cfg(feature = "codegen")]
#[expect(
    clippy::option_if_let_else,
    clippy::panic,
    clippy::match_same_arms,
    clippy::redundant_closure_for_method_calls,
    clippy::single_match_else,
    clippy::too_many_lines,
    reason = "codegen output written like the rest of pumpkin-codegen; panics reject unknown vanilla data at build time"
)]
mod tokens {
    use proc_macro2::TokenStream;
    use quote::{ToTokens, quote};

    use super::{
        HolderSetJson, PoolAliasBindingStruct, StructurePlacementStruct,
        StructurePlacementTypeStruct, StructureSetStruct, StructureStruct, WeightedEntryStruct,
        WeightedGroupStruct, WeightedTargetStruct,
    };

    impl ToTokens for StructureSetStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let placement = &self.placement;
            let structures = &self.structures;

            tokens.extend(quote!(
                StructureSet {
                    placement: #placement,
                    structures: &[#(#structures),*],
                }
            ));
        }
    }

    impl ToTokens for WeightedEntryStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let structure = structure_key_to_token(&self.structure);
            let weight = self.weight;

            tokens.extend(quote!(
                WeightedEntry {
                    structure: #structure,
                    weight: #weight,
                }
            ));
        }
    }

    impl ToTokens for StructurePlacementStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let frequency_reduction = if let Some(method) = &self.frequency_reduction_method {
                let method_token = frequency_reduction_to_token(method);
                quote!(Some(#method_token))
            } else {
                quote!(None)
            };

            let frequency = if let Some(f) = self.frequency {
                quote!(Some(#f))
            } else {
                quote!(None)
            };

            let exclusion_zone = if let Some(ez) = &self.exclusion_zone {
                let other_set = &ez.other_set;
                let chunk_count = ez.chunk_count;
                quote!(Some(ExclusionZone {
                    other_set: #other_set,
                    chunk_count: #chunk_count,
                }))
            } else {
                quote!(None)
            };

            let salt = self.salt;
            let placement_type = &self.r#type;

            tokens.extend(quote!(
                StructurePlacement {
                    frequency_reduction_method: #frequency_reduction,
                    frequency: #frequency,
                    salt: #salt,
                    exclusion_zone: #exclusion_zone,
                    placement_type: #placement_type,
                }
            ));
        }
    }

    impl ToTokens for StructurePlacementTypeStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            match self {
                Self::RandomSpread {
                    spacing,
                    separation,
                    spread_type,
                } => {
                    let spread_type_token = if let Some(st) = spread_type {
                        let st_token = spread_type_to_token(st);
                        quote!(Some(#st_token))
                    } else {
                        quote!(None)
                    };

                    tokens.extend(quote!(
                        StructurePlacementType::RandomSpread(RandomSpreadStructurePlacement {
                            spacing: #spacing,
                            separation: #separation,
                            spread_type: #spread_type_token,
                        })
                    ));
                }
                Self::ConcentricRings {
                    spread,
                    distance,
                    count,
                    preferred_biomes,
                } => {
                    tokens.extend(quote!(
                        StructurePlacementType::ConcentricRings(ConcentricRingsStructurePlacement {
                            spread: #spread,
                            distance: #distance,
                            count: #count,
                            preferred_biomes: #preferred_biomes,
                        })
                    ));
                }
            }
        }
    }

    impl ToTokens for PoolAliasBindingStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            match self {
                Self::Direct { alias, target } => {
                    tokens.extend(quote!(
                        PoolAliasBinding::Direct {
                            alias: #alias,
                            target: #target,
                        }
                    ));
                }
                Self::Random { alias, targets } => {
                    tokens.extend(quote!(
                        PoolAliasBinding::Random {
                            alias: #alias,
                            targets: &[#(#targets),*],
                        }
                    ));
                }
                Self::RandomGroup { groups } => {
                    tokens.extend(quote!(
                        PoolAliasBinding::RandomGroup {
                            groups: &[#(#groups),*],
                        }
                    ));
                }
            }
        }
    }

    impl ToTokens for WeightedTargetStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let target = &self.data;
            let weight = self.weight;
            tokens.extend(quote!(
                WeightedPoolAliasTarget {
                    target: #target,
                    weight: #weight,
                }
            ));
        }
    }

    impl ToTokens for WeightedGroupStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let bindings = &self.data;
            let weight = self.weight;
            tokens.extend(quote!(
                WeightedPoolAliasGroup {
                    bindings: &[#(#bindings),*],
                    weight: #weight,
                }
            ));
        }
    }

    impl ToTokens for StructureStruct {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let HolderSetJson::One(biomes) = &self.biomes else {
                panic!("vanilla structures name their biomes with a tag");
            };
            let step = generation_step_to_token(&self.step);
            let start_pool = if let Some(pool) = &self.start_pool {
                quote!(Some(#pool))
            } else {
                quote!(None)
            };
            let start_jigsaw_name = if let Some(name) = &self.start_jigsaw_name {
                quote!(Some(#name))
            } else {
                quote!(None)
            };
            let size = if let Some(s) = self.size {
                quote!(Some(#s))
            } else {
                quote!(None)
            };
            let terrain_adaptation = if let Some(ta) = &self.terrain_adaptation {
                let ta_token = terrain_adaptation_to_token(ta);
                quote!(#ta_token)
            } else {
                quote!(TerrainAdaptation::None)
            };
            let project_start_to_heightmap = if let Some(ps) = &self.project_start_to_heightmap {
                quote!(Some(#ps))
            } else {
                quote!(None)
            };
            let max_distance_from_center = if let Some(m) = self.max_distance_from_center {
                let (horizontal, vertical) = m.axes();
                quote!(Some(MaxDistance { horizontal: #horizontal, vertical: #vertical }))
            } else {
                quote!(None)
            };
            let liquid_settings = if let Some(ls) = &self.liquid_settings {
                quote!(Some(#ls))
            } else {
                quote!(None)
            };
            let dimension_padding = if let Some(dp) = self.dimension_padding {
                quote!(Some(#dp))
            } else {
                quote!(None)
            };
            let use_expansion_hack = if let Some(hack) = self.use_expansion_hack {
                quote!(Some(#hack))
            } else {
                quote!(None)
            };
            let pool_aliases = &self.pool_aliases;

            let height_val = self.start_height.as_ref().or(self.height.as_ref());
            let start_height = if let Some(hv) = height_val {
                let hp = value_to_height_provider(hv);
                quote!(Some(#hp))
            } else {
                quote!(None)
            };

            let spawn_overrides: Vec<TokenStream> = self
                .spawn_overrides
                .iter()
                .map(|(category, so)| {
                    let bbox = match so.bounding_box.as_str() {
                        "piece" => quote!(BoundingBoxType::Piece),
                        _ => quote!(BoundingBoxType::Full),
                    };
                    let spawns: Vec<TokenStream> = so
                        .spawns
                        .iter()
                        .map(|entry| {
                            let et = &entry.entity_type;
                            let min = entry.min_count;
                            let max = entry.max_count;
                            let weight = entry.weight;
                            quote!(SpawnEntry {
                                entity_type: #et,
                                min_count: #min,
                                max_count: #max,
                                weight: #weight,
                            })
                        })
                        .collect();
                    quote!(SpawnOverride {
                        category: #category,
                        bounding_box: #bbox,
                        spawns: &[#(#spawns),*],
                    })
                })
                .collect();

            let is_beached = if let Some(ib) = self.is_beached {
                quote!(Some(#ib))
            } else {
                quote!(None)
            };
            let mineshaft_type = if let Some(mt) = &self.mineshaft_type {
                quote!(Some(#mt))
            } else {
                quote!(None)
            };
            let biome_temp = if let Some(bt) = &self.biome_temp {
                quote!(Some(#bt))
            } else {
                quote!(None)
            };
            let cluster_probability = if let Some(cp) = self.cluster_probability {
                quote!(Some(#cp))
            } else {
                quote!(None)
            };
            let large_probability = if let Some(lp) = self.large_probability {
                quote!(Some(#lp))
            } else {
                quote!(None)
            };

            let structure_type = structure_type_to_token(&self.structure_type);

            tokens.extend(quote!(
                Structure {
                    biomes: #biomes,
                    step: #step,
                    start_pool: #start_pool,
                    start_jigsaw_name: #start_jigsaw_name,
                    size: #size,
                    terrain_adaptation: #terrain_adaptation,
                    start_height: #start_height,
                    project_start_to_heightmap: #project_start_to_heightmap,
                    max_distance_from_center: #max_distance_from_center,
                    liquid_settings: #liquid_settings,
                    dimension_padding: #dimension_padding,
                    use_expansion_hack: #use_expansion_hack,
                    pool_aliases: &[#(#pool_aliases),*],
                    spawn_overrides: &[#(#spawn_overrides),*],
                    is_beached: #is_beached,
                    mineshaft_type: #mineshaft_type,
                    biome_temp: #biome_temp,
                    cluster_probability: #cluster_probability,
                    large_probability: #large_probability,
                    structure_type: #structure_type,
                }
            ));
        }
    }

    fn value_to_height_provider(v: &serde_json::Value) -> TokenStream {
        if let Some(type_str) = v.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "minecraft:uniform" => {
                    let min = value_to_y_offset(&v["min_inclusive"]);
                    let max = value_to_y_offset(&v["max_inclusive"]);
                    quote!(HeightProvider::Uniform(UniformHeightProvider {
                        min_inclusive: #min,
                        max_inclusive: #max,
                    }))
                }
                "minecraft:trapezoid" => {
                    let min = value_to_y_offset(&v["min_inclusive"]);
                    let max = value_to_y_offset(&v["max_inclusive"]);
                    let plateau = v.get("plateau").and_then(|p| p.as_i64()).map(|p| p as i32);
                    let plateau_token = if let Some(p) = plateau {
                        quote!(Some(#p))
                    } else {
                        quote!(None)
                    };
                    quote!(HeightProvider::Trapezoid(TrapezoidHeightProvider {
                        min_inclusive: #min,
                        max_inclusive: #max,
                        plateau: #plateau_token,
                    }))
                }
                "minecraft:very_biased_to_bottom" => {
                    let min = value_to_y_offset(&v["min_inclusive"]);
                    let max = value_to_y_offset(&v["max_inclusive"]);
                    let inner = v.get("inner").and_then(|i| i.as_u64()).map(|i| i as u32);
                    let inner_token = if let Some(i) = inner {
                        quote!(std::num::NonZero::new(#i))
                    } else {
                        quote!(None)
                    };
                    quote!(HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeightProvider {
                        min_inclusive: #min,
                        max_inclusive: #max,
                        inner: #inner_token,
                    }))
                }
                "minecraft:constant" => {
                    let y = value_to_y_offset(&v["value"]);
                    quote!(HeightProvider::Uniform(UniformHeightProvider {
                        min_inclusive: #y,
                        max_inclusive: #y,
                    }))
                }
                _ => {
                    let y = value_to_y_offset(v);
                    quote!(HeightProvider::Uniform(UniformHeightProvider {
                        min_inclusive: #y,
                        max_inclusive: #y,
                    }))
                }
            }
        } else {
            let y = value_to_y_offset(v);
            quote!(HeightProvider::Uniform(UniformHeightProvider {
                min_inclusive: #y,
                max_inclusive: #y,
            }))
        }
    }

    fn value_to_y_offset(v: &serde_json::Value) -> TokenStream {
        if let Some(abs) = v.get("absolute") {
            let val = abs.as_i64().unwrap_or(0) as i16;
            quote!(YOffset::Absolute(Absolute { absolute: #val }))
        } else if let Some(ab) = v.get("above_bottom") {
            let val = ab.as_i64().unwrap_or(0) as i8;
            quote!(YOffset::AboveBottom(AboveBottom { above_bottom: #val }))
        } else if let Some(bt) = v.get("below_top") {
            let val = bt.as_i64().unwrap_or(0) as i8;
            quote!(YOffset::BelowTop(BelowTop { below_top: #val }))
        } else if let Some(sl) = v.get("relative_to_sea_level") {
            let val = sl.as_i64().unwrap_or(0) as i16;
            quote!(YOffset::RelativeToSeaLevel(RelativeToSeaLevel { relative_to_sea_level: #val }))
        } else if let Some(val) = v.as_i64() {
            let val = val as i16;
            quote!(YOffset::Absolute(Absolute { absolute: #val }))
        } else {
            quote!(YOffset::Absolute(Absolute { absolute: 0 }))
        }
    }

    // Helper functions

    fn structure_type_to_token(st: &str) -> TokenStream {
        match st {
            "minecraft:jigsaw" => quote!(StructureType::Jigsaw),
            "minecraft:buried_treasure" => quote!(StructureType::BuriedTreasure),
            "minecraft:desert_pyramid" => quote!(StructureType::DesertPyramid),
            "minecraft:end_city" => quote!(StructureType::EndCity),
            "minecraft:fortress" => quote!(StructureType::Fortress),
            "minecraft:igloo" => quote!(StructureType::Igloo),
            "minecraft:jungle_temple" => quote!(StructureType::JungleTemple),
            "minecraft:woodland_mansion" => quote!(StructureType::WoodlandMansion),
            "minecraft:mineshaft" => quote!(StructureType::Mineshaft),
            "minecraft:ocean_monument" => quote!(StructureType::OceanMonument),
            "minecraft:nether_fossil" => quote!(StructureType::NetherFossil),
            "minecraft:ocean_ruin" => quote!(StructureType::OceanRuin),
            "minecraft:ruined_portal" => quote!(StructureType::RuinedPortal),
            "minecraft:shipwreck" => quote!(StructureType::Shipwreck),
            "minecraft:stronghold" => quote!(StructureType::Stronghold),
            "minecraft:swamp_hut" => quote!(StructureType::SwampHut),
            _ => quote!(StructureType::Unknown),
        }
    }

    fn terrain_adaptation_to_token(ta: &str) -> TokenStream {
        match ta {
            "beard_thin" => quote!(TerrainAdaptation::BeardThin),
            "beard_box" => quote!(TerrainAdaptation::BeardBox),
            "bury" => quote!(TerrainAdaptation::Bury),
            "encapsulate" => quote!(TerrainAdaptation::Encapsulate),
            _ => quote!(TerrainAdaptation::None),
        }
    }

    #[must_use]
    pub fn structure_key_to_token(key: &str) -> TokenStream {
        let stripped = key.strip_prefix("minecraft:").unwrap_or(key);

        match stripped {
            "pillager_outpost" => quote!(StructureKeys::PillagerOutpost),
            "mineshaft" => quote!(StructureKeys::Mineshaft),
            "mineshaft_mesa" => quote!(StructureKeys::MineshaftMesa),
            "mansion" => quote!(StructureKeys::Mansion),
            "jungle_pyramid" => quote!(StructureKeys::JunglePyramid),
            "desert_pyramid" => quote!(StructureKeys::DesertPyramid),
            "igloo" => quote!(StructureKeys::Igloo),
            "shipwreck" => quote!(StructureKeys::Shipwreck),
            "shipwreck_beached" => quote!(StructureKeys::ShipwreckBeached),
            "swamp_hut" => quote!(StructureKeys::SwampHut),
            "stronghold" => quote!(StructureKeys::Stronghold),
            "monument" => quote!(StructureKeys::Monument),
            "ocean_ruin_cold" => quote!(StructureKeys::OceanRuinCold),
            "ocean_ruin_warm" => quote!(StructureKeys::OceanRuinWarm),
            "fortress" => quote!(StructureKeys::Fortress),
            "nether_fossil" => quote!(StructureKeys::NetherFossil),
            "end_city" => quote!(StructureKeys::EndCity),
            "buried_treasure" => quote!(StructureKeys::BuriedTreasure),
            "bastion_remnant" => quote!(StructureKeys::BastionRemnant),
            "village_plains" => quote!(StructureKeys::VillagePlains),
            "village_desert" => quote!(StructureKeys::VillageDesert),
            "village_savanna" => quote!(StructureKeys::VillageSavanna),
            "village_snowy" => quote!(StructureKeys::VillageSnowy),
            "village_taiga" => quote!(StructureKeys::VillageTaiga),
            "ruined_portal" => quote!(StructureKeys::RuinedPortal),
            "ruined_portal_desert" => quote!(StructureKeys::RuinedPortalDesert),
            "ruined_portal_jungle" => quote!(StructureKeys::RuinedPortalJungle),
            "ruined_portal_swamp" => quote!(StructureKeys::RuinedPortalSwamp),
            "ruined_portal_mountain" => quote!(StructureKeys::RuinedPortalMountain),
            "ruined_portal_ocean" => quote!(StructureKeys::RuinedPortalOcean),
            "ruined_portal_nether" => quote!(StructureKeys::RuinedPortalNether),
            "ancient_city" => quote!(StructureKeys::AncientCity),
            "trail_ruins" => quote!(StructureKeys::TrailRuins),
            "trial_chambers" => quote!(StructureKeys::TrialChambers),
            "abandoned_camp_bamboo_jungle" => quote!(StructureKeys::AbandonedCampBambooJungle),
            "abandoned_camp_birch_forest" => quote!(StructureKeys::AbandonedCampBirchForest),
            "abandoned_camp_cherry_grove" => quote!(StructureKeys::AbandonedCampCherryGrove),
            "abandoned_camp_dappled_forest" => quote!(StructureKeys::AbandonedCampDappledForest),
            "abandoned_camp_flower_forest" => quote!(StructureKeys::AbandonedCampFlowerForest),
            "abandoned_camp_forest" => quote!(StructureKeys::AbandonedCampForest),
            "abandoned_camp_meadow" => quote!(StructureKeys::AbandonedCampMeadow),
            "abandoned_camp_old_growth_birch_forest" => {
                quote!(StructureKeys::AbandonedCampOldGrowthBirchForest)
            }
            "abandoned_camp_old_growth_pine_taiga" => {
                quote!(StructureKeys::AbandonedCampOldGrowthPineTaiga)
            }
            "abandoned_camp_old_growth_spruce_taiga" => {
                quote!(StructureKeys::AbandonedCampOldGrowthSpruceTaiga)
            }
            "abandoned_camp_pale_garden" => quote!(StructureKeys::AbandonedCampPaleGarden),
            "abandoned_camp_savanna" => quote!(StructureKeys::AbandonedCampSavanna),
            "abandoned_camp_snowy_taiga" => quote!(StructureKeys::AbandonedCampSnowyTaiga),
            "abandoned_camp_sparse_jungle" => quote!(StructureKeys::AbandonedCampSparseJungle),
            "abandoned_camp_swamp" => quote!(StructureKeys::AbandonedCampSwamp),
            "abandoned_camp_taiga" => quote!(StructureKeys::AbandonedCampTaiga),
            "abandoned_camp_windswept_forest" => {
                quote!(StructureKeys::AbandonedCampWindsweptForest)
            }
            "abandoned_camp_wooded_badlands" => quote!(StructureKeys::AbandonedCampWoodedBadlands),
            _ => panic!("Unknown structure key: {stripped}"),
        }
    }

    fn frequency_reduction_to_token(method: &str) -> TokenStream {
        match method {
            "default" => quote!(FrequencyReductionMethod::Default),
            "legacy_type_1" => quote!(FrequencyReductionMethod::LegacyType1),
            "legacy_type_2" => quote!(FrequencyReductionMethod::LegacyType2),
            "legacy_type_3" => quote!(FrequencyReductionMethod::LegacyType3),
            _ => quote!(FrequencyReductionMethod::Default),
        }
    }

    fn spread_type_to_token(spread: &str) -> TokenStream {
        match spread {
            "linear" => quote!(SpreadType::Linear),
            "triangular" => quote!(SpreadType::Triangular),
            _ => quote!(SpreadType::Linear),
        }
    }

    fn generation_step_to_token(step: &str) -> TokenStream {
        match step {
            "raw_generation" => quote!(GenerationStep::RawGeneration),
            "lakes" => quote!(GenerationStep::Lakes),
            "local_modifications" => quote!(GenerationStep::LocalModifications),
            "underground_structures" => quote!(GenerationStep::UndergroundStructures),
            "surface_structures" => quote!(GenerationStep::SurfaceStructures),
            "strongholds" => quote!(GenerationStep::Strongholds),
            "underground_ores" => quote!(GenerationStep::UndergroundOres),
            "underground_decoration" => quote!(GenerationStep::UndergroundDecoration),
            "fluid_springs" => quote!(GenerationStep::FluidSprings),
            "vegetal_decoration" => quote!(GenerationStep::VegetalDecoration),
            "top_layer_modification" => quote!(GenerationStep::TopLayerModification),
            _ => panic!("Unknown generation step: {step}"),
        }
    }
}

/// `DimensionType.Y_SIZE`, the default vertical jigsaw reach.
pub const Y_SIZE: i32 = (1 << 12) - 32;

/// A jigsaw `max_distance_from_center`: one value for both axes, or separate ones.
#[derive(Deserialize, Clone, Copy)]
#[serde(untagged)]
pub enum MaxDistanceJson {
    Value(i32),
    Full {
        horizontal: i32,
        #[serde(default = "y_size")]
        vertical: i32,
    },
}

const fn y_size() -> i32 {
    Y_SIZE
}

impl MaxDistanceJson {
    /// `(horizontal, vertical)`, where a single value covers both like `MaxDistance(int)`.
    #[must_use]
    pub const fn axes(self) -> (i32, i32) {
        match self {
            Self::Value(value) => (value, value),
            Self::Full {
                horizontal,
                vertical,
            } => (horizontal, vertical),
        }
    }
}

/// A registry holder set: `#tag`, one ID, or a list of IDs.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum HolderSetJson {
    One(String),
    Many(Vec<String>),
}

impl HolderSetJson {
    /// The entries, with a `#` in front of a tag.
    #[must_use]
    pub fn entries(&self) -> Vec<&str> {
        match self {
            Self::One(entry) => vec![entry.as_str()],
            Self::Many(entries) => entries.iter().map(String::as_str).collect(),
        }
    }
}

#[cfg(feature = "runtime")]
pub use runtime::generation_step;

#[cfg(feature = "runtime")]
mod runtime {
    use pumpkin_data::structures::{
        BoundingBoxType, ConcentricRingsStructurePlacement, ExclusionZone,
        FrequencyReductionMethod, GenerationStep, HeightProvider, MaxDistance, PoolAliasBinding,
        RandomSpreadStructurePlacement, SpawnEntry, SpawnOverride, SpreadType, Structure,
        StructureKeys, StructurePlacement, StructurePlacementType, StructureSet, StructureType,
        TerrainAdaptation, TrapezoidHeightProvider, UniformHeightProvider,
        VeryBiasedToBottomHeightProvider, WeightedEntry, WeightedPoolAliasGroup,
        WeightedPoolAliasTarget,
    };
    use pumpkin_util::y_offset::{AboveBottom, Absolute, BelowTop, RelativeToSeaLevel, YOffset};
    use serde_json::Value;

    use super::{
        HolderSetJson, PoolAliasBindingStruct, StructurePlacementTypeStruct, StructureSetStruct,
        StructureStruct,
    };

    fn leak(value: String) -> &'static str {
        Box::leak(value.into_boxed_str())
    }

    fn leak_slice<T>(values: Vec<T>) -> &'static [T] {
        Box::leak(values.into_boxed_slice())
    }

    pub fn generation_step(name: &str) -> Result<GenerationStep, String> {
        Ok(match name {
            "raw_generation" => GenerationStep::RawGeneration,
            "lakes" => GenerationStep::Lakes,
            "local_modifications" => GenerationStep::LocalModifications,
            "underground_structures" => GenerationStep::UndergroundStructures,
            "surface_structures" => GenerationStep::SurfaceStructures,
            "strongholds" => GenerationStep::Strongholds,
            "underground_ores" => GenerationStep::UndergroundOres,
            "underground_decoration" => GenerationStep::UndergroundDecoration,
            "fluid_springs" => GenerationStep::FluidSprings,
            "vegetal_decoration" => GenerationStep::VegetalDecoration,
            "top_layer_modification" => GenerationStep::TopLayerModification,
            _ => return Err(format!("unknown generation step {name}")),
        })
    }

    fn terrain_adaptation(name: Option<&str>) -> Result<TerrainAdaptation, String> {
        Ok(match name {
            None | Some("none") => TerrainAdaptation::None,
            Some("beard_thin") => TerrainAdaptation::BeardThin,
            Some("beard_box") => TerrainAdaptation::BeardBox,
            Some("bury") => TerrainAdaptation::Bury,
            Some("encapsulate") => TerrainAdaptation::Encapsulate,
            Some(other) => return Err(format!("unknown terrain adaptation {other}")),
        })
    }

    fn structure_type(name: &str) -> StructureType {
        match name.strip_prefix("minecraft:").unwrap_or(name) {
            "jigsaw" => StructureType::Jigsaw,
            "buried_treasure" => StructureType::BuriedTreasure,
            "desert_pyramid" => StructureType::DesertPyramid,
            "end_city" => StructureType::EndCity,
            "fortress" => StructureType::Fortress,
            "igloo" => StructureType::Igloo,
            "jungle_temple" => StructureType::JungleTemple,
            "woodland_mansion" => StructureType::WoodlandMansion,
            "mineshaft" => StructureType::Mineshaft,
            "ocean_monument" => StructureType::OceanMonument,
            "nether_fossil" => StructureType::NetherFossil,
            "ocean_ruin" => StructureType::OceanRuin,
            "ruined_portal" => StructureType::RuinedPortal,
            "shipwreck" => StructureType::Shipwreck,
            "stronghold" => StructureType::Stronghold,
            "swamp_hut" => StructureType::SwampHut,
            _ => StructureType::Unknown,
        }
    }

    fn y_offset(value: &Value) -> Result<YOffset, String> {
        let int = |key: &str| value.get(key).and_then(Value::as_i64);
        let small = |value: i64| i8::try_from(value).map_err(|_| format!("Y offset {value}"));
        let wide = |value: i64| i16::try_from(value).map_err(|_| format!("Y offset {value}"));
        Ok(
            if let Some(absolute) = int("absolute").or_else(|| value.as_i64()) {
                YOffset::Absolute(Absolute {
                    absolute: wide(absolute)?,
                })
            } else if let Some(above_bottom) = int("above_bottom") {
                YOffset::AboveBottom(AboveBottom {
                    above_bottom: small(above_bottom)?,
                })
            } else if let Some(below_top) = int("below_top") {
                YOffset::BelowTop(BelowTop {
                    below_top: small(below_top)?,
                })
            } else if let Some(sea_level) = int("relative_to_sea_level") {
                YOffset::RelativeToSeaLevel(RelativeToSeaLevel {
                    relative_to_sea_level: wide(sea_level)?,
                })
            } else {
                return Err(format!("invalid vertical anchor {value}"));
            },
        )
    }

    /// A structure height provider; a bare anchor or `constant` is a one-value uniform range.
    fn height_provider(value: &Value) -> Result<HeightProvider, String> {
        let range = || -> Result<(YOffset, YOffset), String> {
            Ok((
                y_offset(&value["min_inclusive"])?,
                y_offset(&value["max_inclusive"])?,
            ))
        };
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| kind.strip_prefix("minecraft:").unwrap_or(kind));
        Ok(match kind {
            Some("uniform") => {
                let (min_inclusive, max_inclusive) = range()?;
                HeightProvider::Uniform(UniformHeightProvider {
                    min_inclusive,
                    max_inclusive,
                })
            }
            Some("trapezoid") => {
                let (min_inclusive, max_inclusive) = range()?;
                HeightProvider::Trapezoid(TrapezoidHeightProvider {
                    min_inclusive,
                    max_inclusive,
                    plateau: value
                        .get("plateau")
                        .and_then(Value::as_i64)
                        .map(i32::try_from)
                        .transpose()
                        .map_err(|error| error.to_string())?,
                })
            }
            Some("very_biased_to_bottom") => {
                let (min_inclusive, max_inclusive) = range()?;
                HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeightProvider {
                    min_inclusive,
                    max_inclusive,
                    inner: value
                        .get("inner")
                        .and_then(Value::as_u64)
                        .and_then(|inner| u32::try_from(inner).ok())
                        .and_then(std::num::NonZero::new),
                })
            }
            Some("constant") => {
                let y = y_offset(&value["value"])?;
                HeightProvider::Uniform(UniformHeightProvider {
                    min_inclusive: y,
                    max_inclusive: y,
                })
            }
            Some(other) => return Err(format!("unsupported height provider {other}")),
            None => {
                let y = y_offset(value)?;
                HeightProvider::Uniform(UniformHeightProvider {
                    min_inclusive: y,
                    max_inclusive: y,
                })
            }
        })
    }

    fn pool_alias(binding: PoolAliasBindingStruct) -> PoolAliasBinding {
        match binding {
            PoolAliasBindingStruct::Direct { alias, target } => PoolAliasBinding::Direct {
                alias: leak(alias),
                target: leak(target),
            },
            PoolAliasBindingStruct::Random { alias, targets } => PoolAliasBinding::Random {
                alias: leak(alias),
                targets: leak_slice(
                    targets
                        .into_iter()
                        .map(|target| WeightedPoolAliasTarget {
                            target: leak(target.data),
                            weight: target.weight,
                        })
                        .collect(),
                ),
            },
            PoolAliasBindingStruct::RandomGroup { groups } => PoolAliasBinding::RandomGroup {
                groups: leak_slice(
                    groups
                        .into_iter()
                        .map(|group| WeightedPoolAliasGroup {
                            bindings: leak_slice(group.data.into_iter().map(pool_alias).collect()),
                            weight: group.weight,
                        })
                        .collect(),
                ),
            },
        }
    }

    fn spawn_override(category: String, spawn: super::SpawnOverrideStruct) -> SpawnOverride {
        SpawnOverride {
            category: leak(category),
            bounding_box: if spawn.bounding_box == "piece" {
                BoundingBoxType::Piece
            } else {
                BoundingBoxType::Full
            },
            spawns: leak_slice(
                spawn
                    .spawns
                    .into_iter()
                    .map(|entry| SpawnEntry {
                        entity_type: leak(entry.entity_type),
                        min_count: entry.min_count,
                        max_count: entry.max_count,
                        weight: entry.weight,
                    })
                    .collect(),
            ),
        }
    }

    impl StructureStruct {
        /// Builds the engine structure. Only a tag survives in its `biomes` text; callers
        /// resolve the biome IDs.
        pub fn build(self) -> Result<Structure, String> {
            let biomes = match &self.biomes {
                HolderSetJson::One(tag) if tag.starts_with('#') => leak(tag.clone()),
                _ => "",
            };
            let height = self.start_height.as_ref().or(self.height.as_ref());
            Ok(Structure {
                biomes,
                step: generation_step(&self.step)?,
                start_pool: self.start_pool.map(leak),
                start_jigsaw_name: self.start_jigsaw_name.map(leak),
                size: self.size,
                terrain_adaptation: terrain_adaptation(self.terrain_adaptation.as_deref())?,
                start_height: height.map(height_provider).transpose()?,
                project_start_to_heightmap: self.project_start_to_heightmap.map(leak),
                max_distance_from_center: self.max_distance_from_center.map(|m| {
                    let (horizontal, vertical) = m.axes();
                    MaxDistance {
                        horizontal,
                        vertical,
                    }
                }),
                liquid_settings: self.liquid_settings.map(leak),
                dimension_padding: self.dimension_padding,
                use_expansion_hack: self.use_expansion_hack,
                pool_aliases: leak_slice(self.pool_aliases.into_iter().map(pool_alias).collect()),
                spawn_overrides: leak_slice(
                    self.spawn_overrides
                        .into_iter()
                        .map(|(category, spawn)| spawn_override(category, spawn))
                        .collect(),
                ),
                is_beached: self.is_beached,
                mineshaft_type: self.mineshaft_type.map(leak),
                biome_temp: self.biome_temp.map(leak),
                cluster_probability: self.cluster_probability,
                large_probability: self.large_probability,
                structure_type: structure_type(&self.structure_type),
            })
        }
    }

    fn placement_type(
        placement: StructurePlacementTypeStruct,
    ) -> Result<StructurePlacementType, String> {
        Ok(match placement {
            StructurePlacementTypeStruct::RandomSpread {
                spacing,
                separation,
                spread_type,
            } => {
                if separation >= spacing {
                    return Err("spacing has to be larger than separation".into());
                }
                StructurePlacementType::RandomSpread(RandomSpreadStructurePlacement {
                    spacing,
                    separation,
                    spread_type: match spread_type.as_deref() {
                        None => None,
                        Some("linear") => Some(SpreadType::Linear),
                        Some("triangular") => Some(SpreadType::Triangular),
                        Some(other) => return Err(format!("unknown spread type {other}")),
                    },
                })
            }
            StructurePlacementTypeStruct::ConcentricRings {
                spread,
                distance,
                count,
                preferred_biomes,
            } => StructurePlacementType::ConcentricRings(ConcentricRingsStructurePlacement {
                spread,
                distance,
                count,
                preferred_biomes: leak(preferred_biomes),
            }),
        })
    }

    impl StructureSetStruct {
        /// Builds the engine set, resolving each structure ID to its key.
        pub fn build(
            self,
            mut key: impl FnMut(&str) -> Option<StructureKeys>,
        ) -> Result<StructureSet, String> {
            let placement = self.placement;
            let structures = self
                .structures
                .into_iter()
                .map(|entry| {
                    Ok(WeightedEntry {
                        structure: key(&entry.structure)
                            .ok_or_else(|| format!("no key for structure {}", entry.structure))?,
                        weight: entry.weight,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if structures.is_empty() {
                return Err("structure set has no structures".into());
            }
            let frequency_reduction_method = placement
                .frequency_reduction_method
                .as_deref()
                .map(|method| match method {
                    "default" => Ok(FrequencyReductionMethod::Default),
                    "legacy_type_1" => Ok(FrequencyReductionMethod::LegacyType1),
                    "legacy_type_2" => Ok(FrequencyReductionMethod::LegacyType2),
                    "legacy_type_3" => Ok(FrequencyReductionMethod::LegacyType3),
                    other => Err(format!("unknown frequency reduction method {other}")),
                })
                .transpose()?;
            Ok(StructureSet {
                placement: StructurePlacement {
                    frequency_reduction_method,
                    frequency: placement.frequency,
                    salt: placement.salt,
                    exclusion_zone: placement.exclusion_zone.map(|zone| ExclusionZone {
                        other_set: leak(zone.other_set),
                        chunk_count: zone.chunk_count,
                    }),
                    placement_type: placement_type(placement.r#type)?,
                },
                structures: leak_slice(structures),
            })
        }
    }
}
