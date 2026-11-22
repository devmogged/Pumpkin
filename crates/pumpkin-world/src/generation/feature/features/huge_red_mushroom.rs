use pumpkin_data::PropertyKey;
use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use super::huge_brown_mushroom::{HugeMushroom, with_faces};
use crate::generation::proto_chunk::GenerationCache;
use crate::world::WorldPortalExt;

/// Java's `HugeRedMushroomFeature`.
pub struct HugeRedMushroomFeature(pub HugeMushroom);

impl HugeRedMushroomFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        min_y: i8,
        height: u16,
        random: &mut RandomGenerator,
        origin: BlockPos,
    ) -> bool {
        let mushroom = &self.0;
        let foliage_radius = mushroom.foliage_radius;
        let tree_height = HugeMushroom::tree_height(random);
        if !mushroom.is_valid_position(
            block_registry,
            chunk,
            min_y,
            height,
            origin,
            tree_height,
            |dy| {
                if (dy < tree_height && dy >= tree_height - 3) || dy == tree_height {
                    foliage_radius
                } else {
                    0
                }
            },
        ) {
            return false;
        }

        let center = foliage_radius - 2;
        for dy in (tree_height - 3)..=tree_height {
            let radius = if dy < tree_height {
                foliage_radius
            } else {
                foliage_radius - 1
            };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let x_edge = dx == -radius || dx == radius;
                    let z_edge = dz == -radius || dz == radius;
                    if dy < tree_height && x_edge == z_edge {
                        continue;
                    }
                    let state = with_faces(
                        mushroom.cap_state(block_registry, chunk, random, origin),
                        &[
                            (PropertyKey::Up, dy >= tree_height - 1),
                            (PropertyKey::West, dx < -center),
                            (PropertyKey::East, dx > center),
                            (PropertyKey::North, dz < -center),
                            (PropertyKey::South, dz > center),
                        ],
                    );
                    let pos = BlockPos::new(origin.0.x + dx, origin.0.y + dy, origin.0.z + dz);
                    HugeMushroom::place_block(chunk, &pos, state);
                }
            }
        }

        mushroom.place_trunk(block_registry, chunk, random, origin, tree_height);
        true
    }
}
