use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::{collections::BTreeMap, fs};

use pumpkin_worldgen_model::structure::{
    StructureSetStruct, StructureStruct, structure_key_to_token,
};

/// Reads structure and structure_set files from 26.3 datapack and emits the complete structures `TokenStream`.
pub fn build() -> TokenStream {
    let structures_dir =
        std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/structure");
    let mut structures_json: BTreeMap<String, StructureStruct> = BTreeMap::new();
    let mut s_entries: Vec<_> = fs::read_dir(structures_dir)
        .expect("Missing worldgen/structure directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    s_entries.sort_by_key(|e| e.path());
    for entry in s_entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let content = fs::read_to_string(&path).expect("Failed to read structure file");
        let structure: StructureStruct =
            serde_json::from_str(&content).expect("Failed to parse structure JSON");
        structures_json.insert(stem, structure);
    }

    let structure_sets_dir =
        std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/structure_set");
    let mut structure_sets_json: BTreeMap<String, StructureSetStruct> = BTreeMap::new();
    let mut ss_entries: Vec<_> = fs::read_dir(structure_sets_dir)
        .expect("Missing worldgen/structure_set directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    ss_entries.sort_by_key(|e| e.path());
    for entry in ss_entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let content = fs::read_to_string(&path).expect("Failed to read structure_set file");
        let set: StructureSetStruct =
            serde_json::from_str(&content).expect("Failed to parse structure_set JSON");
        structure_sets_json.insert(stem, set);
    }

    let mut structure_const_defs = TokenStream::new();
    let mut structure_lookup_arms = TokenStream::new();
    let mut structure_from_name_arms = Vec::new();
    let mut structure_to_name_arms = Vec::new();
    let mut structure_all_names: Vec<String> = Vec::new();

    for (name, structure) in &structures_json {
        let stripped_name = name.strip_prefix("minecraft:").unwrap_or(name);
        let upper_name = stripped_name.to_uppercase();
        let const_name = format_ident!("{}", upper_name);
        let variant_ident = format_ident!("{}", stripped_name.to_pascal_case());
        let key_variant = structure_key_to_token(name);

        structure_const_defs.extend(quote!(
            pub const #const_name: Self = #structure;
        ));

        structure_lookup_arms.extend(quote!(
            #key_variant => &Self::#const_name,
        ));

        structure_from_name_arms.push(quote! {
            #stripped_name => Some(Self::#variant_ident),
        });
        structure_to_name_arms.push(quote! {
            Self::#variant_ident => #name,
        });
        structure_all_names.push(stripped_name.to_string());
    }

    let mut structure_set_const_defs = TokenStream::new();
    let mut all_structure_set_idents = Vec::new();
    let mut all_structure_set_names = Vec::new();

    for (name, structure_set) in &structure_sets_json {
        let stripped_name = name.strip_prefix("minecraft:").unwrap_or(name);
        let upper_name = stripped_name.to_uppercase().replace('/', "_");
        let const_name = format_ident!("{}", upper_name);

        structure_set_const_defs.extend(quote!(
            pub const #const_name: Self = #structure_set;
        ));

        all_structure_set_idents.push(const_name);
        all_structure_set_names.push(stripped_name.to_string());
    }

    let structure_all_names_tokens: Vec<TokenStream> = structure_all_names
        .iter()
        .map(|name| {
            let full = format!("minecraft:{name}");
            quote! { #full }
        })
        .collect();

    quote!(
        use pumpkin_util::math::floor_div;
        use pumpkin_util::random::{
            RandomGenerator, RandomImpl, get_carver_seed, get_region_seed,
            legacy_rand::LegacyRand, xoroshiro128::Xoroshiro,
        };
        use pumpkin_util::y_offset::{AboveBottom, Absolute, BelowTop, RelativeToSeaLevel, YOffset};

        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum StructureKeys {
            PillagerOutpost,
            Mineshaft,
            MineshaftMesa,
            Mansion,
            JunglePyramid,
            DesertPyramid,
            Igloo,
            Shipwreck,
            ShipwreckBeached,
            SwampHut,
            Stronghold,
            Monument,
            OceanRuinCold,
            OceanRuinWarm,
            Fortress,
            NetherFossil,
            EndCity,
            BuriedTreasure,
            BastionRemnant,
            VillagePlains,
            VillageDesert,
            VillageSavanna,
            VillageSnowy,
            VillageTaiga,
            RuinedPortal,
            RuinedPortalDesert,
            RuinedPortalJungle,
            RuinedPortalSwamp,
            RuinedPortalMountain,
            RuinedPortalOcean,
            RuinedPortalNether,
            AncientCity,
            TrailRuins,
            TrialChambers,
            AbandonedCampBambooJungle,
            AbandonedCampBirchForest,
            AbandonedCampCherryGrove,
            AbandonedCampDappledForest,
            AbandonedCampFlowerForest,
            AbandonedCampForest,
            AbandonedCampMeadow,
            AbandonedCampOldGrowthBirchForest,
            AbandonedCampOldGrowthPineTaiga,
            AbandonedCampOldGrowthSpruceTaiga,
            AbandonedCampPaleGarden,
            AbandonedCampSavanna,
            AbandonedCampSnowyTaiga,
            AbandonedCampSparseJungle,
            AbandonedCampSwamp,
            AbandonedCampTaiga,
            AbandonedCampWindsweptForest,
            AbandonedCampWoodedBadlands,
            /// A structure a datapack added, numbered by [`crate::structure_registry`].
            Custom(u16),
        }

        impl StructureKeys {
            /// Every vanilla structure ID.
            pub const VANILLA_NAMES: &'static [&'static str] = &[#(#structure_all_names_tokens),*];

            /// The vanilla structure with this path in the `minecraft` namespace.
            #[must_use]
            pub fn vanilla(path: &str) -> Option<Self> {
                match path {
                    #(#structure_from_name_arms)*
                    _ => None,
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                Self::vanilla(name.strip_prefix("minecraft:").unwrap_or(name))
                    .or_else(|| crate::structure_registry::custom_key(name))
            }

            /// The path of a vanilla structure, or the full ID of a datapack one.
            #[must_use]
            pub fn to_name(&self) -> &'static str {
                match self {
                    #(#structure_to_name_arms)*
                    Self::Custom(index) => crate::structure_registry::custom_name(*index),
                }
            }

            /// Every structure ID, including the ones datapacks added.
            #[must_use]
            pub fn all_names() -> &'static [&'static str] {
                crate::structure_registry::names().unwrap_or(Self::VANILLA_NAMES)
            }
        }

        pub struct StructureSet {
            pub placement: StructurePlacement,
            pub structures: &'static [WeightedEntry],
        }

        #[derive(Clone)]
        pub struct WeightedEntry {
            pub structure: StructureKeys,
            pub weight: u32,
        }

        pub struct ExclusionZone {
            pub other_set: &'static str,
            pub chunk_count: i32,
        }

        pub struct StructurePlacement {
            pub locate_offset: [i32; 3],
            pub frequency_reduction_method: Option<FrequencyReductionMethod>,
            pub frequency: Option<f32>,
            pub salt: u32,
            pub exclusion_zone: Option<ExclusionZone>,
            pub placement_type: StructurePlacementType,
        }

        #[derive(Clone, Copy)]
        pub enum FrequencyReductionMethod {
            Default,
            LegacyType1,
            LegacyType2,
            LegacyType3,
        }

        pub enum StructurePlacementType {
            RandomSpread(RandomSpreadStructurePlacement),
            ConcentricRings(ConcentricRingsStructurePlacement),
        }

        pub struct RandomSpreadStructurePlacement {
            pub spacing: i32,
            pub separation: i32,
            pub spread_type: Option<SpreadType>,
        }

        pub struct ConcentricRingsStructurePlacement {
            pub spread: i32,
            pub distance: i32,
            pub count: i32,
            pub preferred_biomes: &'static str,
        }

        #[derive(Clone, Copy)]
        pub enum SpreadType {
            Linear,
            Triangular,
        }

        impl SpreadType {
            pub fn get(&self, random: &mut RandomGenerator, bound: i32) -> i32 {
                match self {
                    Self::Linear => random.next_bounded_i32(bound),
                    Self::Triangular => i32::midpoint(
                        random.next_bounded_i32(bound),
                        random.next_bounded_i32(bound),
                    ),
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum TerrainAdaptation {
            None,
            BeardThin,
            BeardBox,
            Bury,
            Encapsulate,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum StructureType {
            Jigsaw,
            BuriedTreasure,
            DesertPyramid,
            EndCity,
            Fortress,
            Igloo,
            JungleTemple,
            WoodlandMansion,
            Mineshaft,
            OceanMonument,
            NetherFossil,
            OceanRuin,
            RuinedPortal,
            Shipwreck,
            Stronghold,
            SwampHut,
            Unknown,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum BoundingBoxType {
            Piece,
            Full,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct SpawnEntry {
            pub entity_type: &'static str,
            pub min_count: u32,
            pub max_count: u32,
            pub weight: u32,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct SpawnOverride {
            pub category: &'static str,
            pub bounding_box: BoundingBoxType,
            pub spawns: &'static [SpawnEntry],
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum HeightProvider {
            Uniform(UniformHeightProvider),
            Trapezoid(TrapezoidHeightProvider),
            VeryBiasedToBottom(VeryBiasedToBottomHeightProvider),
        }

        impl HeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                match self {
                    Self::Uniform(provider) => provider.get(random, min_y, height),
                    Self::Trapezoid(provider) => provider.get(random, min_y, height),
                    Self::VeryBiasedToBottom(provider) => provider.get(random, min_y, height),
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct UniformHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
        }

        impl UniformHeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                let min = self.min_inclusive.get_y(min_y as i16, height);
                let max = self.max_inclusive.get_y(min_y as i16, height);
                if min >= max {
                    min
                } else {
                    random.next_inbetween_i32(min, max)
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct TrapezoidHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
            pub plateau: Option<i32>,
        }

        impl TrapezoidHeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                let plateau = self.plateau.unwrap_or(0);
                let i = self.min_inclusive.get_y(min_y as i16, height);
                let j = self.max_inclusive.get_y(min_y as i16, height);

                if i >= j {
                    return i;
                }

                let k = j - i;
                if plateau >= k {
                    return random.next_inbetween_i32(i, j);
                }

                let l = (k - plateau) / 2;
                let m = k - l;
                i + random.next_inbetween_i32(0, m) + random.next_inbetween_i32(0, l)
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct VeryBiasedToBottomHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
            pub inner: Option<std::num::NonZero<u32>>,
        }

        impl VeryBiasedToBottomHeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                let min = self.min_inclusive.get_y(min_y as i16, height);
                let max = self.max_inclusive.get_y(min_y as i16, height);
                let inner = self.inner.map_or(1, std::num::NonZero::get) as i32;

                if max - min - inner + 1 <= 0 {
                    return min;
                }

                let upper_inclusive = random.next_inbetween_i32(min + inner, max);
                let biased_upper_inclusive = random.next_inbetween_i32(min, upper_inclusive - 1);
                random.next_inbetween_i32(min, biased_upper_inclusive - 1 + inner)
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum PoolAliasBinding {
            Direct {
                alias: &'static str,
                target: &'static str,
            },
            Random {
                alias: &'static str,
                targets: &'static [WeightedPoolAliasTarget],
            },
            RandomGroup {
                groups: &'static [WeightedPoolAliasGroup],
            },
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct WeightedPoolAliasTarget {
            pub target: &'static str,
            pub weight: u32,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct WeightedPoolAliasGroup {
            pub bindings: &'static [PoolAliasBinding],
            pub weight: u32,
        }

        /// How far jigsaw pieces may reach from the start, per axis.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct MaxDistance {
            pub horizontal: i32,
            pub vertical: i32,
        }

        impl MaxDistance {
            /// One reach for both axes, like vanilla's `MaxDistance(int)`.
            #[must_use]
            pub const fn new(value: i32) -> Self {
                Self { horizontal: value, vertical: value }
            }
        }

        pub struct Structure {
            pub biomes: &'static str,
            pub step: GenerationStep,
            pub start_pool: Option<&'static str>,
            pub start_jigsaw_name: Option<&'static str>,
            pub size: Option<i32>,
            pub terrain_adaptation: TerrainAdaptation,
            pub start_height: Option<HeightProvider>,
            pub project_start_to_heightmap: Option<&'static str>,
            pub max_distance_from_center: Option<MaxDistance>,
            pub liquid_settings: Option<&'static str>,
            pub dimension_padding: Option<i32>,
            pub use_expansion_hack: Option<bool>,
            pub pool_aliases: &'static [PoolAliasBinding],
            pub spawn_overrides: &'static [SpawnOverride],
            pub is_beached: Option<bool>,
            pub mineshaft_type: Option<&'static str>,
            pub biome_temp: Option<&'static str>,
            pub cluster_probability: Option<f32>,
            pub large_probability: Option<f32>,
            pub structure_type: StructureType,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum GenerationStep {
            RawGeneration,
            Lakes,
            LocalModifications,
            UndergroundStructures,
            SurfaceStructures,
            Strongholds,
            UndergroundOres,
            UndergroundDecoration,
            FluidSprings,
            VegetalDecoration,
            TopLayerModification,
        }

        impl GenerationStep {
            #[must_use]
            pub const fn ordinal(&self) -> usize {
                *self as usize
            }
        }

        pub struct StructurePlacementCalculator {
            pub seed: i64,
        }

        impl StructurePlacementCalculator {
            #[must_use]
            pub const fn new(seed: i64) -> Self {
                Self { seed }
            }
        }

        impl Structure {
            #structure_const_defs

            /// Stands in for a custom structure that a set names but no file defines. It
            /// allows no biomes, so it never starts.
            pub const UNDEFINED: Self = Structure {
                biomes: "",
                step: GenerationStep::SurfaceStructures,
                start_pool: None,
                start_jigsaw_name: None,
                size: None,
                terrain_adaptation: TerrainAdaptation::None,
                start_height: None,
                project_start_to_heightmap: None,
                max_distance_from_center: None,
                liquid_settings: None,
                dimension_padding: None,
                use_expansion_hack: None,
                pool_aliases: &[],
                spawn_overrides: &[],
                is_beached: None,
                mineshaft_type: None,
                biome_temp: None,
                cluster_probability: None,
                large_probability: None,
                structure_type: StructureType::Unknown,
            };

            /// The generated vanilla definition.
            #[must_use]
            pub const fn vanilla(key: &StructureKeys) -> &'static Self {
                match *key {
                    #structure_lookup_arms
                    StructureKeys::Custom(_) => &Self::UNDEFINED,
                }
            }

            /// The active definition, including datapack replacements.
            #[must_use]
            pub fn get(key: &StructureKeys) -> &'static Self {
                crate::structure_registry::structure(*key).unwrap_or_else(|| Self::vanilla(key))
            }
        }

        impl StructureSet {
            #structure_set_const_defs

            /// The generated vanilla sets, in the same order as [`Self::NAMES`].
            pub const VANILLA: &'static [&'static StructureSet] = &[
                #(&Self::#all_structure_set_idents),*
            ];

            /// The registry names of the vanilla structure sets, without the `minecraft:` prefix.
            pub const NAMES: &'static [&'static str] = &[
                #(#all_structure_set_names),*
            ];

            /// Every active set, vanilla ones first at their generated index.
            #[must_use]
            pub fn all() -> &'static [&'static Self] {
                crate::structure_registry::sets().map_or(Self::VANILLA, |(sets, _)| sets)
            }

            /// The names of [`Self::all`], without the `minecraft:` prefix for vanilla's namespace.
            #[must_use]
            pub fn names() -> &'static [&'static str] {
                crate::structure_registry::sets().map_or(Self::NAMES, |(_, names)| names)
            }

            #[must_use]
            pub fn get(name: &str) -> Option<&'static Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                let index = Self::names().iter().position(|existing| *existing == name)?;
                Self::all().get(index).copied()
            }
        }
    )
}
