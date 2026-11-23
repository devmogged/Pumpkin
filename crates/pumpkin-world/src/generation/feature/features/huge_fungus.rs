use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl},
};

use super::weeping_vines::WeepingVinesFeature;
use crate::{
    generation::{block_predicate::BlockPredicate, proto_chunk::GenerationCache},
    world::WorldPortalExt,
};

pub struct HugeFungusFeature {
    pub valid_base_block: &'static BlockState,
    pub stem_state: &'static BlockState,
    pub hat_state: &'static BlockState,
    pub decor_state: &'static BlockState,
    pub replaceable_blocks: BlockPredicate,
    pub planted: bool,
}

impl HugeFungusFeature {
    #[allow(clippy::too_many_arguments)]
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        block_registry: &dyn WorldPortalExt,
        _min_y: i8,
        height: u16,
        _feature: pumpkin_data::placed_feature::PlacedFeature,
        random: &mut RandomGenerator,
        pos: BlockPos,
    ) -> bool {
        let below = pos.down();
        let below_block = GenerationCache::get_block_state(chunk, &below.0).to_block_id();
        let valid_base_id = Block::from_state_id(self.valid_base_block.id).id;
        if below_block != valid_base_id {
            return false;
        }

        let mut total_height = random.next_inbetween_i32(4, 13);
        if random.next_bounded_i32(12) == 0 {
            total_height *= 2;
        }

        if !self.planted && pos.0.y + total_height + 1 >= i32::from(height) {
            return false;
        }

        let is_huge = !self.planted && random.next_f32() < 0.06;
        chunk.set_block_state(&pos.0, Block::AIR.default_state);
        self.place_stem(chunk, block_registry, random, pos, total_height, is_huge);
        self.place_hat(chunk, block_registry, random, pos, total_height, is_huge);
        true
    }

    fn is_replaceable<T: GenerationCache>(
        &self,
        chunk: &T,
        block_registry: &dyn WorldPortalExt,
        pos: BlockPos,
        check_non_replaceable_plants: bool,
    ) -> bool {
        GenerationCache::get_block_state(chunk, &pos.0)
            .to_state()
            .replaceable()
            || (check_non_replaceable_plants
                && self.replaceable_blocks.test(block_registry, chunk, &pos))
    }

    fn place_stem<T: GenerationCache>(
        &self,
        chunk: &mut T,
        block_registry: &dyn WorldPortalExt,
        random: &mut RandomGenerator,
        pos: BlockPos,
        total_height: i32,
        is_huge: bool,
    ) {
        let stem_radius = i32::from(is_huge);

        for dx in -stem_radius..=stem_radius {
            for dz in -stem_radius..=stem_radius {
                let corner_of_huge_stem =
                    is_huge && dx.abs() == stem_radius && dz.abs() == stem_radius;

                for dy in 0..total_height {
                    let block_pos = pos.add(dx, dy, dz);
                    if self.is_replaceable(chunk, block_registry, block_pos, true) {
                        if self.planted {
                            chunk.set_block_state(&block_pos.0, self.stem_state);
                        } else if corner_of_huge_stem {
                            if random.next_f32() < 0.1 {
                                chunk.set_block_state(&block_pos.0, self.stem_state);
                            }
                        } else {
                            chunk.set_block_state(&block_pos.0, self.stem_state);
                        }
                    }
                }
            }
        }
    }

    fn place_hat<T: GenerationCache>(
        &self,
        chunk: &mut T,
        block_registry: &dyn WorldPortalExt,
        random: &mut RandomGenerator,
        pos: BlockPos,
        total_height: i32,
        is_huge: bool,
    ) {
        let place_vines = Block::from_state_id(self.hat_state.id).id == Block::NETHER_WART_BLOCK.id;
        let hat_height = (random.next_bounded_i32(1 + total_height / 3) + 5).min(total_height);
        let hat_start_y = total_height - hat_height;

        for dy in hat_start_y..=total_height {
            let rnd = random.next_bounded_i32(3);
            let mut radius = if hat_height > 8 && dy < hat_start_y + 4 {
                3
            } else if dy < total_height - rnd {
                2
            } else {
                1
            };
            if is_huge {
                radius += 1;
            }

            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let is_edge_x = dx == -radius || dx == radius;
                    let is_edge_z = dz == -radius || dz == radius;
                    let inside = !is_edge_x && !is_edge_z && dy != total_height;
                    let corner = is_edge_x && is_edge_z;
                    let is_hat_bottom = dy < hat_start_y + 3;
                    let block_pos = pos.add(dx, dy, dz);

                    if self.is_replaceable(chunk, block_registry, block_pos, false) {
                        if is_hat_bottom {
                            if !inside {
                                self.place_hat_drop_block(chunk, random, block_pos, place_vines);
                            }
                        } else if inside {
                            self.place_hat_block(
                                chunk,
                                random,
                                block_pos,
                                0.1,
                                0.2,
                                if place_vines { 0.1 } else { 0.0 },
                            );
                        } else if corner {
                            self.place_hat_block(
                                chunk,
                                random,
                                block_pos,
                                0.01,
                                0.7,
                                if place_vines { 0.083 } else { 0.0 },
                            );
                        } else {
                            self.place_hat_block(
                                chunk,
                                random,
                                block_pos,
                                5.0e-4,
                                0.98,
                                if place_vines { 0.07 } else { 0.0 },
                            );
                        }
                    }
                }
            }
        }
    }

    fn place_hat_block<T: GenerationCache>(
        &self,
        chunk: &mut T,
        random: &mut RandomGenerator,
        block_pos: BlockPos,
        decor_prob: f32,
        hat_prob: f32,
        vines_prob: f32,
    ) {
        if random.next_f32() < decor_prob {
            chunk.set_block_state(&block_pos.0, self.decor_state);
        } else if random.next_f32() < hat_prob {
            chunk.set_block_state(&block_pos.0, self.hat_state);
            if random.next_f32() < vines_prob {
                Self::try_place_weeping_vines(chunk, random, block_pos);
            }
        }
    }

    fn place_hat_drop_block<T: GenerationCache>(
        &self,
        chunk: &mut T,
        random: &mut RandomGenerator,
        block_pos: BlockPos,
        place_vines: bool,
    ) {
        let below = block_pos.down();
        let below_id = GenerationCache::get_block_state(chunk, &below.0).to_block_id();
        let hat_id = Block::from_state_id(self.hat_state.id).id;

        if below_id == hat_id {
            chunk.set_block_state(&block_pos.0, self.hat_state);
        } else if random.next_f32() < 0.15 {
            chunk.set_block_state(&block_pos.0, self.hat_state);
            if place_vines && random.next_bounded_i32(11) == 0 {
                Self::try_place_weeping_vines(chunk, random, block_pos);
            }
        }
    }

    fn try_place_weeping_vines<T: GenerationCache>(
        chunk: &mut T,
        random: &mut RandomGenerator,
        hat_block_pos: BlockPos,
    ) {
        let place_pos = hat_block_pos.down();
        if chunk.is_air(&place_pos.0) {
            let mut goal_vine_height = random.next_inbetween_i32(1, 5);
            if random.next_bounded_i32(7) == 0 {
                goal_vine_height *= 2;
            }

            WeepingVinesFeature::place_weeping_vines_column(
                chunk,
                random,
                place_pos,
                goal_vine_height,
                23,
                25,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::{
        configured_feature::ConfiguredFeature as ConfiguredId, dimension::Dimension,
        placed_feature::PlacedFeature,
    };
    use pumpkin_util::{random::worldgen_random::WorldgenRandom, world_seed::Seed};

    use crate::generation::{
        feature::configured_features::{CONFIGURED_FEATURES, ConfiguredFeature},
        get_world_gen,
        proto_chunk::ProtoChunk,
        proto_chunk_test::stage_parity::TestBlockRegistry,
    };

    use super::*;

    fn grow_crimson_fungus(y: i32, roots_above: bool) -> (bool, ProtoChunk) {
        let generator = get_world_gen(
            Seed(42),
            Dimension::THE_NETHER,
            false,
            Vec::new(),
            String::new(),
        );
        let mut chunk = ProtoChunk::new(0, 0, &generator);
        chunk.set_block_state(8, y - 1, 8, Block::CRIMSON_NYLIUM.default_state);
        if roots_above {
            chunk.set_block_state(8, y + 1, 8, Block::CRIMSON_ROOTS.default_state);
        }
        let height = chunk.generation_height();
        let ConfiguredFeature::HugeFungus(feature) =
            &CONFIGURED_FEATURES[&ConfiguredId::CrimsonFungus]
        else {
            panic!("crimson fungus must be a huge fungus feature");
        };
        let placed = feature.generate(
            &mut chunk,
            &TestBlockRegistry,
            0,
            height,
            PlacedFeature::CrimsonFungi,
            &mut RandomGenerator::Worldgen(WorldgenRandom::from_seed(42)),
            BlockPos::new(8, y, 8),
        );
        (placed, chunk)
    }

    #[test]
    fn wild_huge_fungus_respects_the_generator_ceiling() {
        let (placed, chunk) = grow_crimson_fungus(124, false);
        assert!(!placed);
        assert!(chunk.is_air(&BlockPos::new(8, 124, 8).0));
    }

    #[test]
    fn huge_fungus_stem_replaces_crimson_roots() {
        let (placed, chunk) = grow_crimson_fungus(65, true);
        assert!(placed);
        assert_eq!(
            chunk
                .get_block_state(&BlockPos::new(8, 66, 8).0)
                .to_block_id(),
            Block::CRIMSON_STEM.id,
        );
    }
}
