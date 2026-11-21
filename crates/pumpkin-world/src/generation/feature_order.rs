use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use pumpkin_data::chunk::Biome;
use pumpkin_data::placed_feature::PlacedFeature;
use rustc_hash::FxHashSet;

use crate::{biome::ActiveBiomeSupplier, world_info::BiomeSource};

pub struct FeatureOrder {
    biomes: HashSet<u8>,
    per_step: Vec<Vec<PlacedFeature>>,
}

impl FeatureOrder {
    pub fn for_source(
        supplier: &ActiveBiomeSupplier,
        source: Option<&BiomeSource>,
        possible_biomes: &FxHashSet<u16>,
    ) -> Option<Self> {
        let mut biomes = match supplier {
            ActiveBiomeSupplier::DirectMultiNoise(_) => {
                let Some(BiomeSource::WithBiomes { biomes, .. }) = source else {
                    return None;
                };
                biomes
                    .as_array()?
                    .iter()
                    .filter_map(|entry| {
                        let id = entry.get("biome")?.as_str()?;
                        Biome::from_name(id.strip_prefix("minecraft:").unwrap_or(id))
                    })
                    .collect()
            }
            ActiveBiomeSupplier::Checkerboard { biomes, .. } => biomes.to_vec(),
            ActiveBiomeSupplier::Fixed(supplier) => vec![supplier.biome],
            ActiveBiomeSupplier::End(_) => overridden_biomes(END_BIOMES)?,
            ActiveBiomeSupplier::MultiNoise(_) => {
                let stock = if possible_biomes.contains(&u16::from(Biome::NETHER_WASTES.id)) {
                    NETHER_BIOMES
                } else {
                    OVERWORLD_BIOMES
                };
                overridden_biomes(stock)?
            }
        };
        let mut biome_ids = HashSet::with_capacity(biomes.len());
        biomes.retain(|biome| biome_ids.insert(biome.id));
        let per_step = sort_features_per_step(&biomes).unwrap_or_else(|feature| {
            tracing::warn!(
                "Skipping feature generation after ordering cycle at {}",
                feature.to_name()
            );
            Vec::new()
        });
        Some(Self {
            biomes: biome_ids,
            per_step,
        })
    }

    pub fn select(&self, biome_ids: &[u8], step: usize) -> Vec<(usize, PlacedFeature)> {
        select_features_for_dimension(biome_ids, step, &self.biomes, &self.per_step)
    }
}

fn overridden_biomes(vanilla: &[&Biome]) -> Option<Vec<&'static Biome>> {
    let biomes = vanilla
        .iter()
        .filter_map(|biome| Biome::from_id(biome.id))
        .collect::<Vec<_>>();
    biomes
        .iter()
        .zip(vanilla)
        .any(|(actual, stock)| actual.features != stock.features)
        .then_some(biomes)
}

// The order of biomes in this array is used by FeatureSorter to break ties between otherwise independent features.
const OVERWORLD_BIOMES: &[&Biome] = &[
    &Biome::MUSHROOM_FIELDS,
    &Biome::DEEP_FROZEN_OCEAN,
    &Biome::FROZEN_OCEAN,
    &Biome::DEEP_COLD_OCEAN,
    &Biome::COLD_OCEAN,
    &Biome::DEEP_OCEAN,
    &Biome::OCEAN,
    &Biome::DEEP_LUKEWARM_OCEAN,
    &Biome::LUKEWARM_OCEAN,
    &Biome::WARM_OCEAN,
    &Biome::STONY_SHORE,
    &Biome::SWAMP,
    &Biome::MANGROVE_SWAMP,
    &Biome::SNOWY_SLOPES,
    &Biome::SNOWY_PLAINS,
    &Biome::SNOWY_BEACH,
    &Biome::WINDSWEPT_GRAVELLY_HILLS,
    &Biome::GROVE,
    &Biome::WINDSWEPT_HILLS,
    &Biome::SNOWY_TAIGA,
    &Biome::WINDSWEPT_FOREST,
    &Biome::TAIGA,
    &Biome::PLAINS,
    &Biome::MEADOW,
    &Biome::BEACH,
    &Biome::FOREST,
    &Biome::OLD_GROWTH_SPRUCE_TAIGA,
    &Biome::FLOWER_FOREST,
    &Biome::BIRCH_FOREST,
    &Biome::DARK_FOREST,
    &Biome::PALE_GARDEN,
    &Biome::SAVANNA_PLATEAU,
    &Biome::SAVANNA,
    &Biome::JUNGLE,
    &Biome::BADLANDS,
    &Biome::DESERT,
    &Biome::WOODED_BADLANDS,
    &Biome::JAGGED_PEAKS,
    &Biome::STONY_PEAKS,
    &Biome::FROZEN_RIVER,
    &Biome::RIVER,
    &Biome::ICE_SPIKES,
    &Biome::DAPPLED_FOREST,
    &Biome::OLD_GROWTH_PINE_TAIGA,
    &Biome::SUNFLOWER_PLAINS,
    &Biome::OLD_GROWTH_BIRCH_FOREST,
    &Biome::SPARSE_JUNGLE,
    &Biome::BAMBOO_JUNGLE,
    &Biome::ERODED_BADLANDS,
    &Biome::WINDSWEPT_SAVANNA,
    &Biome::CHERRY_GROVE,
    &Biome::FROZEN_PEAKS,
    &Biome::DRIPSTONE_CAVES,
    &Biome::LUSH_CAVES,
    &Biome::SULFUR_CAVES,
    &Biome::DEEP_DARK,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FeatureData {
    step: usize,
    encounter_index: usize,
    feature: PlacedFeature,
}

impl Ord for FeatureData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.step, self.encounter_index, self.feature).cmp(&(
            other.step,
            other.encounter_index,
            other.feature,
        ))
    }
}

impl PartialOrd for FeatureData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

const NETHER_BIOMES: &[&Biome] = &[
    &Biome::NETHER_WASTES,
    &Biome::SOUL_SAND_VALLEY,
    &Biome::CRIMSON_FOREST,
    &Biome::WARPED_FOREST,
    &Biome::BASALT_DELTAS,
];

const END_BIOMES: &[&Biome] = &[
    &Biome::THE_END,
    &Biome::END_HIGHLANDS,
    &Biome::END_MIDLANDS,
    &Biome::SMALL_END_ISLANDS,
    &Biome::END_BARRENS,
];

static OVERWORLD_FEATURES_PER_STEP: LazyLock<Vec<Vec<PlacedFeature>>> =
    LazyLock::new(|| sort_features_per_step(OVERWORLD_BIOMES).unwrap_or_default());
static OVERWORLD_BIOME_IDS: LazyLock<HashSet<u8>> =
    LazyLock::new(|| OVERWORLD_BIOMES.iter().map(|biome| biome.id).collect());

static NETHER_FEATURES_PER_STEP: LazyLock<Vec<Vec<PlacedFeature>>> =
    LazyLock::new(|| sort_features_per_step(NETHER_BIOMES).unwrap_or_default());
static NETHER_BIOME_IDS: LazyLock<HashSet<u8>> =
    LazyLock::new(|| NETHER_BIOMES.iter().map(|biome| biome.id).collect());

static END_FEATURES_PER_STEP: LazyLock<Vec<Vec<PlacedFeature>>> =
    LazyLock::new(|| sort_features_per_step(END_BIOMES).unwrap_or_default());
static END_BIOME_IDS: LazyLock<HashSet<u8>> =
    LazyLock::new(|| END_BIOMES.iter().map(|biome| biome.id).collect());

pub fn select_features(biome_ids: &[u8], step: usize) -> Vec<(usize, PlacedFeature)> {
    if biome_ids.iter().any(|b| NETHER_BIOME_IDS.contains(b)) {
        select_features_for_dimension(
            biome_ids,
            step,
            &NETHER_BIOME_IDS,
            &NETHER_FEATURES_PER_STEP,
        )
    } else if biome_ids.iter().any(|b| END_BIOME_IDS.contains(b)) {
        select_features_for_dimension(biome_ids, step, &END_BIOME_IDS, &END_FEATURES_PER_STEP)
    } else {
        select_features_for_dimension(
            biome_ids,
            step,
            &OVERWORLD_BIOME_IDS,
            &OVERWORLD_FEATURES_PER_STEP,
        )
    }
}

fn select_features_for_dimension(
    biome_ids: &[u8],
    step: usize,
    allowed_biomes: &HashSet<u8>,
    features_per_step: &[Vec<PlacedFeature>],
) -> Vec<(usize, PlacedFeature)> {
    let mut selected = HashSet::new();

    for biome_id in biome_ids {
        if !allowed_biomes.contains(biome_id) {
            continue;
        }
        if let Some(features) = Biome::from_id(*biome_id).and_then(|biome| biome.features.get(step))
        {
            selected.extend(features.iter().copied());
        }
    }

    features_per_step
        .get(step)
        .into_iter()
        .flatten()
        .copied()
        .enumerate()
        .filter(|(_, feature)| selected.contains(feature))
        .collect()
}

fn sort_features_per_step(biomes: &[&Biome]) -> Result<Vec<Vec<PlacedFeature>>, PlacedFeature> {
    let mut feature_indices = HashMap::new();
    let mut edges: BTreeMap<FeatureData, BTreeSet<FeatureData>> = BTreeMap::new();
    let mut max_steps = 0;

    for biome in biomes {
        max_steps = max_steps.max(biome.features.len());
        let mut biome_features = Vec::new();

        for (step, features) in biome.features.iter().enumerate() {
            for &feature in *features {
                let next_index = feature_indices.len();
                let encounter_index = *feature_indices.entry(feature).or_insert(next_index);
                let data = FeatureData {
                    step,
                    encounter_index,
                    feature,
                };
                edges.entry(data).or_default();
                biome_features.push(data);
            }
        }

        for pair in biome_features.windows(2) {
            edges.entry(pair[0]).or_default().insert(pair[1]);
        }
    }

    let mut discovered = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    let mut sorted = Vec::with_capacity(edges.len());
    for &feature in edges.keys() {
        visit_feature(feature, &edges, &mut discovered, &mut visiting, &mut sorted)?;
    }
    sorted.reverse();

    let mut per_step = vec![Vec::new(); max_steps];
    for feature in sorted {
        per_step[feature.step].push(feature.feature);
    }
    Ok(per_step)
}

fn visit_feature(
    feature: FeatureData,
    edges: &BTreeMap<FeatureData, BTreeSet<FeatureData>>,
    discovered: &mut BTreeSet<FeatureData>,
    visiting: &mut BTreeSet<FeatureData>,
    sorted: &mut Vec<FeatureData>,
) -> Result<(), PlacedFeature> {
    if discovered.contains(&feature) {
        return Ok(());
    }
    if !visiting.insert(feature) {
        return Err(feature.feature);
    }
    if let Some(next_features) = edges.get(&feature) {
        for &next in next_features {
            visit_feature(next, edges, discovered, visiting, sorted)?;
        }
    }
    visiting.remove(&feature);
    discovered.insert(feature);
    sorted.push(feature);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_grass_uses_vanilla_global_feature_index() {
        let step = &OVERWORLD_FEATURES_PER_STEP[9];
        assert_eq!(
            step.iter()
                .position(|feature| *feature == PlacedFeature::PatchDryGrassDesert),
            Some(72)
        );
        assert_eq!(
            step.iter()
                .position(|feature| *feature == PlacedFeature::PatchDryGrassBadlands),
            Some(74)
        );
        assert_eq!(
            step.iter()
                .position(|feature| *feature == PlacedFeature::TreesJungle),
            Some(10)
        );
    }

    #[test]
    fn biome_selection_keeps_global_indices_and_order() {
        let selected = select_features(&[Biome::SAVANNA.id, Biome::DESERT.id], 9);
        assert!(selected.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(selected.contains(&(72, PlacedFeature::PatchDryGrassDesert)));
        assert!(
            !selected
                .iter()
                .any(|(_, feature)| { *feature == PlacedFeature::PatchDryGrassBadlands })
        );

        let savanna_grass = selected
            .iter()
            .find(|(_, feature)| *feature == PlacedFeature::PatchGrassSavanna)
            .expect("savanna grass must be selected");
        assert_eq!(savanna_grass.0, 15);
    }

    #[test]
    fn selection_deduplicates_biomes_from_neighbor_chunks() {
        let once = select_features(&[Biome::DESERT.id], 9);
        let repeated = select_features(&[Biome::DESERT.id, Biome::DESERT.id, Biome::DESERT.id], 9);
        assert_eq!(once, repeated);
    }

    #[test]
    fn non_overworld_selection_preserves_local_indexing() {
        let selected = select_features(&[Biome::NETHER_WASTES.id], 9);
        assert!(!selected.is_empty());
        assert!(
            selected
                .iter()
                .enumerate()
                .all(|(index, (feature_index, _))| index == *feature_index)
        );
    }
}
