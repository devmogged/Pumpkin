use pumpkin_data::tag::Taggable;
use pumpkin_data::{Block, BlockState, Property, PropertyKey, tag};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl},
};

use crate::generation::{
    block_predicate::BlockPredicate, block_state_provider::BlockStateProvider,
    proto_chunk::GenerationCache,
};
use crate::world::WorldPortalExt;

/// The shared parts of Java's `AbstractHugeMushroomFeature`.
pub struct HugeMushroom {
    pub cap_provider: BlockStateProvider,
    pub stem_provider: BlockStateProvider,
    pub foliage_radius: i32,
    pub can_place_on: BlockPredicate,
}

impl HugeMushroom {
    #[must_use]
    pub fn tree_height(random: &mut RandomGenerator) -> i32 {
        let mut height = random.next_bounded_i32(3) + 4;
        if random.next_bounded_i32(12) == 0 {
            height *= 2;
        }
        height
    }

    pub fn place_block<T: GenerationCache>(
        chunk: &mut T,
        pos: &BlockPos,
        state: &'static BlockState,
    ) {
        let (current_block, current_state) = chunk.get_block_and_state(pos);
        if current_state.is_air()
            || current_block.has_tag(&tag::Block::MINECRAFT_REPLACEABLE_BY_MUSHROOMS)
        {
            chunk.set_block_state(&pos.0, state);
        }
    }

    #[expect(clippy::too_many_arguments)]
    pub fn is_valid_position<T: GenerationCache>(
        &self,
        block_registry: &dyn WorldPortalExt,
        chunk: &T,
        min_y: i8,
        height: u16,
        origin: BlockPos,
        tree_height: i32,
        radius_for_height: impl Fn(i32) -> i32,
    ) -> bool {
        let min_y = i32::from(min_y);
        // LevelHeightAccessor.getMaxY is the highest block, inclusive.
        let max_y = min_y + i32::from(height) - 1;
        let y = origin.0.y;
        if y < min_y + 1 || y + tree_height + 1 > max_y {
            return false;
        }
        if !self
            .can_place_on
            .test(block_registry, chunk, &origin.down())
        {
            return false;
        }
        for dy in 0..=tree_height {
            let radius = radius_for_height(dy);
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let pos = BlockPos::new(origin.0.x + dx, y + dy, origin.0.z + dz);
                    let (block, state) = chunk.get_block_and_state(&pos);
                    if !state.is_air() && !block.has_tag(&tag::Block::MINECRAFT_LEAVES) {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn cap_state<T: GenerationCache>(
        &self,
        block_registry: &dyn WorldPortalExt,
        chunk: &T,
        random: &mut RandomGenerator,
        origin: BlockPos,
    ) -> &'static BlockState {
        self.cap_provider.get(random, origin, chunk, block_registry)
    }

    pub fn place_trunk<T: GenerationCache>(
        &self,
        block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        random: &mut RandomGenerator,
        origin: BlockPos,
        tree_height: i32,
    ) {
        for dy in 0..tree_height {
            let state = self
                .stem_provider
                .get(random, origin, chunk, block_registry);
            Self::place_block(chunk, &origin.up_height(dy), state);
        }
    }
}

/// Sets the given boolean properties when the state has all of them, like Java's
/// `hasProperty` guard before `setValue`.
#[must_use]
pub fn with_faces(
    state: &'static BlockState,
    faces: &[(PropertyKey, bool)],
) -> &'static BlockState {
    let block = Block::from_state_id(state.id);
    let Some(properties) = block.properties(state.id) else {
        return state;
    };
    let mut properties = properties.to_props();
    if !faces
        .iter()
        .all(|(key, _)| properties.iter().any(|property| property.key() == *key))
    {
        return state;
    }
    for &(key, value) in faces {
        if let Some(property) = properties.iter_mut().find(|property| property.key() == key)
            && let Some(updated) =
                Property::from_key_value_str(key, if value { "true" } else { "false" })
        {
            *property = updated;
        }
    }
    BlockState::from_id(block.from_properties(&properties).to_state_id(block))
}

/// Java's `HugeBrownMushroomFeature`.
pub struct HugeBrownMushroomFeature(pub HugeMushroom);

impl HugeBrownMushroomFeature {
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
        let radius = mushroom.foliage_radius;
        let tree_height = HugeMushroom::tree_height(random);
        if !mushroom.is_valid_position(
            block_registry,
            chunk,
            min_y,
            height,
            origin,
            tree_height,
            |dy| if dy <= 3 { 0 } else { radius },
        ) {
            return false;
        }

        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let (min_x, max_x) = (dx == -radius, dx == radius);
                let (min_z, max_z) = (dz == -radius, dz == radius);
                let x_edge = min_x || max_x;
                let z_edge = min_z || max_z;
                if x_edge && z_edge {
                    continue;
                }
                let state = with_faces(
                    mushroom.cap_state(block_registry, chunk, random, origin),
                    &[
                        (PropertyKey::West, min_x || (z_edge && dx == 1 - radius)),
                        (PropertyKey::East, max_x || (z_edge && dx == radius - 1)),
                        (PropertyKey::North, min_z || (x_edge && dz == 1 - radius)),
                        (PropertyKey::South, max_z || (x_edge && dz == radius - 1)),
                    ],
                );
                let pos = BlockPos::new(origin.0.x + dx, origin.0.y + tree_height, origin.0.z + dz);
                HugeMushroom::place_block(chunk, &pos, state);
            }
        }

        mushroom.place_trunk(block_registry, chunk, random, origin, tree_height);
        true
    }
}
