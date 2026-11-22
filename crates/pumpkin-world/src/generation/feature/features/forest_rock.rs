use crate::generation::{block_predicate::BlockPredicate, proto_chunk::GenerationCache};
use crate::world::WorldPortalExt;
use pumpkin_data::BlockState;
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl},
};

/// Java's `BlockBlobFeature`.
pub struct ForestRockFeature {
    pub state: &'static BlockState,
    pub can_place_on: BlockPredicate,
}

impl ForestRockFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        random: &mut RandomGenerator,
        pos: BlockPos,
    ) -> bool {
        let mut pos = pos;
        let min_y = i32::from(chunk.bottom_y()) + 3;
        while pos.0.y > min_y && !self.can_place_on.test(block_registry, chunk, &pos.down()) {
            pos = pos.down();
        }
        if pos.0.y <= min_y {
            return false;
        }

        let block_state = self.state;

        for _ in 0..3 {
            let xr = random.next_bounded_i32(2);
            let yr = random.next_bounded_i32(2);
            let zr = random.next_bounded_i32(2);
            let threshold = (xr + yr + zr) as f32 * 0.333f32 + 0.5f32;
            let threshold_sq = threshold * threshold;

            let start = BlockPos::new(pos.0.x - xr, pos.0.y - yr, pos.0.z - zr);
            let end = BlockPos::new(pos.0.x + xr, pos.0.y + yr, pos.0.z + zr);

            for candidate in BlockPos::iterate(start, end) {
                let dx = (candidate.0.x - pos.0.x) as f32;
                let dy = (candidate.0.y - pos.0.y) as f32;
                let dz = (candidate.0.z - pos.0.z) as f32;
                if dx * dx + dy * dy + dz * dz <= threshold_sq {
                    chunk.set_block_state(&candidate.0, block_state);
                }
            }

            pos = pos.add(
                -1 + random.next_bounded_i32(2),
                -random.next_bounded_i32(2),
                -1 + random.next_bounded_i32(2),
            );
        }

        true
    }
}
