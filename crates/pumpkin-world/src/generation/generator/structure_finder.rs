use pumpkin_data::structures::{
    ConcentricRingsStructurePlacement, RandomSpreadStructurePlacement, StructurePlacement,
    StructurePlacementType, StructureSet,
};
use pumpkin_util::math::{floor_div, position::BlockPos};

use crate::generation::structure::placement::{
    GlobalStructureCache, RingLayout, get_structure_chunk_in_region,
};

use super::WorldGenerator;
use crate::ProtoChunk;

/// Block-level position of a found structure plus squared distance from the
/// search origin, used internally to track the running nearest candidate.
#[derive(Debug, Clone)]
pub struct FoundStructure {
    pub pos: BlockPos,
    pub distance_sq: f64,
}

/// The exact placement-defined map location (vanilla StructurePlacement.getLocatePos).
#[must_use]
pub const fn locate_pos(placement: &StructurePlacement, chunk_x: i32, chunk_z: i32) -> BlockPos {
    let [x, y, z] = placement.locate_offset;
    BlockPos::new((chunk_x << 4) + x, y, (chunk_z << 4) + z)
}

/// Finds the block position of the nearest structure whose placement is listed
/// in `placements`, within `max_search_radius` chunk-region rings.
///
/// Mirrors the two-pass logic in vanilla's
/// `ChunkGenerator.findNearestMapStructure`:
///
/// 1. **Concentric-rings** placements (strongholds) are resolved in one pass
///    from the pre-computed [`GlobalStructureCache`].
/// 2. **Random-spread** placements are searched ring-by-ring outward, stopping
///    at the first radius that produces any result.
///
/// The best candidate from both passes is returned.
///
/// Returns [`StructureSearch::Pending`] while the stronghold positions are still
/// being computed, so the caller can retry instead of reporting a wrong result.
pub fn find_nearest_structure(
    origin: BlockPos,
    placements: &[&StructurePlacement],
    max_search_radius: i32,
    world_seed: i64,
    global_cache: &GlobalStructureCache,
) -> StructureSearch {
    if placements.is_empty() {
        return StructureSearch::NotFound;
    }

    let mut nearest: Option<FoundStructure> = None;

    // ── Pass 1: Concentric-rings (strongholds) ──────────────────────────────
    for p in placements {
        if let StructurePlacementType::ConcentricRings(rings) = &p.placement_type {
            match find_nearest_concentric(origin, p, rings, world_seed, global_cache) {
                ConcentricSearch::Pending => return StructureSearch::Pending,
                ConcentricSearch::Done(Some(found))
                    if nearest
                        .as_ref()
                        .is_none_or(|n| found.distance_sq < n.distance_sq) =>
                {
                    nearest = Some(found);
                }
                ConcentricSearch::Done(_) => {}
            }
        }
    }

    let random_spread: Vec<(&StructurePlacement, &RandomSpreadStructurePlacement)> = placements
        .iter()
        .filter_map(|p| {
            if let StructurePlacementType::RandomSpread(r) = &p.placement_type {
                Some((*p, r))
            } else {
                None
            }
        })
        .collect();

    if !random_spread.is_empty() {
        let chunk_origin_x = origin.0.x >> 4;
        let chunk_origin_z = origin.0.z >> 4;

        'radius: for radius in 0..=max_search_radius {
            for (placement, spread) in &random_spread {
                if let Some(found) = find_nearest_random_spread_at_radius(
                    origin,
                    chunk_origin_x,
                    chunk_origin_z,
                    radius,
                    world_seed,
                    placement,
                    spread,
                ) {
                    if nearest
                        .as_ref()
                        .is_none_or(|n| found.distance_sq < n.distance_sq)
                    {
                        nearest = Some(found);
                    }
                    break 'radius;
                }
            }
        }
    }

    nearest.map_or(StructureSearch::NotFound, |f| StructureSearch::Found(f.pos))
}

/// Result of [`find_nearest_structure`].
#[derive(Debug, Clone)]
pub enum StructureSearch {
    Found(BlockPos),
    NotFound,
    Pending,
}

/// Finds the nearest candidate that actually produces one of `target_structures`.
///
/// Returns the position along with which of the targets was found. Explorer maps
/// and `/locate structure` use this instead of pointing at a placement-only
/// candidate whose biome may reject the requested structure.
#[must_use]
pub fn find_nearest_structure_start(
    origin: BlockPos,
    structure_set: &StructureSet,
    target_structures: &[pumpkin_data::structures::StructureKeys],
    max_search_radius: i32,
    generator: &WorldGenerator,
) -> Option<(BlockPos, pumpkin_data::structures::StructureKeys)> {
    find_nearest_structure_start_in_sets(
        origin,
        &[(structure_set, target_structures)],
        max_search_radius,
        generator,
    )
}

/// Searches every placement at each radius before advancing to the next ring.
///
/// Like vanilla `ChunkGenerator.findNearestMapStructure`, each placement reports the first hit
/// in its ring order, and the nearest of those wins.
#[must_use]
pub fn find_nearest_structure_start_in_sets(
    origin: BlockPos,
    searches: &[(&StructureSet, &[pumpkin_data::structures::StructureKeys])],
    max_search_radius: i32,
    generator: &WorldGenerator,
) -> Option<(BlockPos, pumpkin_data::structures::StructureKeys)> {
    let WorldGenerator::Noise(noise_generator) = generator else {
        return None;
    };
    let chunk_origin_x = origin.0.x >> 4;
    let chunk_origin_z = origin.0.z >> 4;
    let world_seed = noise_generator.random_config.seed as i64;

    for radius in 0..=max_search_radius {
        let mut nearest: Option<(FoundStructure, pumpkin_data::structures::StructureKeys)> = None;
        'placement: for &(structure_set, target_structures) in searches {
            let StructurePlacementType::RandomSpread(placement) =
                &structure_set.placement.placement_type
            else {
                continue;
            };
            let region_origin_x = floor_div(chunk_origin_x, placement.spacing);
            let region_origin_z = floor_div(chunk_origin_z, placement.spacing);
            for region_x_offset in -radius..=radius {
                for region_z_offset in -radius..=radius {
                    if region_x_offset.abs() != radius && region_z_offset.abs() != radius {
                        continue;
                    }
                    let (chunk_x, chunk_z) = get_structure_chunk_in_region(
                        placement,
                        world_seed,
                        region_origin_x + region_x_offset,
                        region_origin_z + region_z_offset,
                        structure_set.placement.salt,
                    );
                    if let Some(key) =
                        structure_start_at(noise_generator, structure_set, chunk_x, chunk_z)
                        && target_structures.contains(&key)
                    {
                        let position = locate_pos(&structure_set.placement, chunk_x, chunk_z);
                        let dx = f64::from(position.0.x - origin.0.x);
                        let dz = f64::from(position.0.z - origin.0.z);
                        let found = FoundStructure {
                            pos: position,
                            distance_sq: dx * dx + dz * dz,
                        };
                        if nearest
                            .as_ref()
                            .is_none_or(|(current, _)| found.distance_sq < current.distance_sq)
                        {
                            nearest = Some((found, key));
                        }
                        continue 'placement;
                    }
                }
            }
        }
        if let Some((found, key)) = nearest {
            return Some((found.pos, key));
        }
    }
    None
}

/// Resolve the first valid start in a placement's weighted order without loading a chunk.
/// Callers must only use an enabled set. Concentric ring positions must be ready.
#[must_use]
pub fn structure_start_at(
    generator: &super::VanillaGenerator,
    set: &StructureSet,
    chunk_x: i32,
    chunk_z: i32,
) -> Option<pumpkin_data::structures::StructureKeys> {
    use crate::generation::{
        noise::router::multi_noise_sampler::MultiNoiseSampler,
        structure::{
            height_sampler::NoiseHeightSampler,
            lazily_generate_structure,
            placement::should_generate_structure,
            structures::{StructureGeneratorContext, create_chunk_random},
        },
    };
    use pumpkin_data::structures::Structure;

    let cache = &generator.global_structure_cache;
    if !should_generate_structure(
        &set.placement,
        &generator.structure_calculator,
        chunk_x,
        chunk_z,
        cache,
    ) {
        return None;
    }
    let seed = generator.random_config.seed;
    let order = if set.structures.len() == 1 {
        set.structures.to_vec()
    } else {
        ProtoChunk::weighted_structure_order(set.structures, seed, chunk_x, chunk_z)
    };
    for key in order.iter().map(|entry| entry.structure) {
        let start = cache.get_or_compute_structure_start(key, chunk_x, chunk_z, || {
            let settings = generator.settings;
            let mut height_sampler = NoiseHeightSampler::new(generator);
            let mut biome_sampler = MultiNoiseSampler::generate(&generator.base_router.multi_noise);
            let context = StructureGeneratorContext {
                seed: seed as i64,
                chunk_x,
                chunk_z,
                random: create_chunk_random(seed as i64, chunk_x, chunk_z),
                sea_level: settings.sea_level,
                min_y: (settings.shape.min_y as i32).max(generator.dimension.min_y),
                height: settings.shape.height.min(generator.dimension.height as u16),
                height_sampler: Some(&mut height_sampler),
                structure_key: Some(key),
            };
            lazily_generate_structure(
                &key,
                Structure::get(&key),
                context,
                &generator.biome_supplier,
                &mut biome_sampler,
            )
        });
        if start.is_some() {
            return Some(key);
        }
    }
    None
}

fn find_nearest_concentric(
    origin: BlockPos,
    placement: &StructurePlacement,
    rings: &ConcentricRingsStructurePlacement,
    world_seed: i64,
    global_cache: &GlobalStructureCache,
) -> ConcentricSearch {
    // No block, callers run in server tasks and the tick, which shutdown waits on
    let strongholds = match global_cache.try_get_ring_chunks(world_seed, rings) {
        RingLayout::Ready(strongholds) => strongholds,
        RingLayout::Pending => return ConcentricSearch::Pending,
        RingLayout::Failed => return ConcentricSearch::Done(None),
    };

    let ox = origin.0.x as f64;
    let oz = origin.0.z as f64;

    strongholds
        .iter()
        .map(|(cx, cz)| {
            let pos = locate_pos(placement, *cx, *cz);
            let dx = pos.0.x as f64 - ox;
            let dz = pos.0.z as f64 - oz;
            FoundStructure {
                pos,
                distance_sq: dx * dx + dz * dz,
            }
        })
        .min_by(|a, b| a.distance_sq.total_cmp(&b.distance_sq))
        .map_or(ConcentricSearch::Done(None), |found| {
            ConcentricSearch::Done(Some(found))
        })
}

/// Outcome of a stronghold lookup: the ring positions may still be computing.
enum ConcentricSearch {
    Pending,
    Done(Option<FoundStructure>),
}

fn find_nearest_random_spread_at_radius(
    origin: BlockPos,
    chunk_origin_x: i32,
    chunk_origin_z: i32,
    radius: i32,
    world_seed: i64,
    placement: &StructurePlacement,
    spread: &RandomSpreadStructurePlacement,
) -> Option<FoundStructure> {
    let spacing = spread.spacing;
    let ox = origin.0.x as f64;
    let oz = origin.0.z as f64;

    let mut best: Option<FoundStructure> = None;

    for rx_off in -radius..=radius {
        for rz_off in -radius..=radius {
            if rx_off.abs() != radius && rz_off.abs() != radius {
                continue;
            }

            let rx = floor_div(chunk_origin_x, spacing) + rx_off;
            let rz = floor_div(chunk_origin_z, spacing) + rz_off;

            let (struct_cx, struct_cz) =
                get_structure_chunk_in_region(spread, world_seed, rx, rz, placement.salt);

            let pos = locate_pos(placement, struct_cx, struct_cz);
            let dx = pos.0.x as f64 - ox;
            let dz = pos.0.z as f64 - oz;
            let dist_sq = dx * dx + dz * dz;

            if best.as_ref().is_none_or(|b| dist_sq < b.distance_sq) {
                best = Some(FoundStructure {
                    pos,
                    distance_sq: dist_sq,
                });
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use pumpkin_data::{
        dimension::Dimension,
        structures::{StructureKeys, StructureSet},
    };
    use pumpkin_util::{math::position::BlockPos, world_seed::Seed};

    use super::find_nearest_structure_start;
    use crate::generation::get_world_gen;

    /// Positions from `/locate structure` on a vanilla 26.3 server, seed 20261120.
    #[test]
    fn locates_nether_structures_with_nether_biomes() {
        let world_gen = get_world_gen(
            Seed(20_261_120),
            Dimension::THE_NETHER,
            false,
            Vec::new(),
            String::new(),
        );
        let set = StructureSet::get("nether_complexes").expect("vanilla set");
        for (origin, key, x, z) in [
            ((0, 0), StructureKeys::Fortress, 32, 48),
            ((0, 0), StructureKeys::BastionRemnant, -240, 448),
            ((3000, -2000), StructureKeys::Fortress, 2896, -2032),
            ((3000, -2000), StructureKeys::BastionRemnant, 2400, -2560),
            ((-4000, 5000), StructureKeys::Fortress, -4656, 4512),
            ((-4000, 5000), StructureKeys::BastionRemnant, -4176, 4880),
        ] {
            let origin = BlockPos::new(origin.0, 64, origin.1);
            assert_eq!(
                find_nearest_structure_start(origin, set, &[key], 100, &world_gen),
                Some((BlockPos::new(x, 0, z), key)),
                "{key:?} from {origin:?}"
            );
        }
    }
}
