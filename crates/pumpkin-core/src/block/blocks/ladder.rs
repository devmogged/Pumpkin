use crate::block::{
    BlockBehaviour, BlockIsReplacing, CanPlaceAtArgs, GetStateForNeighborUpdateArgs, OnPlaceArgs,
};
use crate::entity::EntityBase;
use pumpkin_data::BlockStateId;
use pumpkin_data::block_properties::{BlockProperties, Facing, LadderLikeProperties};
use pumpkin_data::fluid::Fluid;
use pumpkin_data::{Block, BlockDirection, FacingExt, HorizontalFacingExt};
use pumpkin_macros::pumpkin_block;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::tick::TickPriority;
use pumpkin_world::world::BlockAccessor;

#[pumpkin_block("minecraft:ladder")]
pub struct LadderBlock;

impl BlockBehaviour for LadderBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        if args.replacing == BlockIsReplacing::None
            || matches!(args.replacing, BlockIsReplacing::Water(_))
        {
            let behind_pos = args.position.offset(args.direction.to_offset());
            let (behind_block, behind_state_id) = args.world.get_block_and_state_id(&behind_pos);
            if behind_block == args.block
                && ladder_properties(behind_block, behind_state_id).is_some_and(|props| {
                    props.facing.to_block_direction() == args.direction.opposite()
                })
            {
                return BlockStateId::AIR;
            }
        }
        let mut props = LadderLikeProperties::default(&Block::LADDER);
        props.waterlogged = args.replacing.water_source();

        let mut directions = args.player.get_entity().get_entity_facing_order();
        if args.replacing == BlockIsReplacing::None
            || matches!(args.replacing, BlockIsReplacing::Water(_))
        {
            let face = args.direction.to_facing();
            if let Some(i) = directions.iter().position(|&d| d == face)
                && i > 0
            {
                directions.copy_within(0..i, 1);
                directions[0] = face;
            }
        }

        for dir in directions {
            if dir == Facing::Up || dir == Facing::Down {
                continue;
            }
            if !can_place_ladder_at(
                args.world,
                args.position,
                dir.to_block_direction(),
                args.block,
            ) {
                continue;
            }
            if let Some(facing) = dir.opposite().to_horizontal_facing() {
                props.facing = facing;
                return if args.block == &Block::LADDER {
                    props.to_state_id(args.block)
                } else {
                    args.block
                        .from_properties(&props.to_props())
                        .to_state_id(args.block)
                };
            }
        }
        Block::AIR.default_state.id
    }

    fn can_place_at(&self, args: CanPlaceAtArgs<'_>) -> bool {
        if args.player.is_none() {
            let Some(props) = ladder_properties(args.block, args.state.id) else {
                return false;
            };
            return can_place_ladder_at(
                args.block_accessor,
                args.position,
                props.facing.to_block_direction().opposite(),
                args.block,
            );
        }
        if let Some(dir) = args.direction {
            let behind_pos = args.position.offset(dir.to_offset());
            let (behind_block, behind_state) = args.block_accessor.get_block_and_state(&behind_pos);
            if behind_block == args.block
                && ladder_properties(behind_block, behind_state.id)
                    .is_some_and(|props| props.facing.to_block_direction() == dir.opposite())
            {
                return false;
            }
        }
        for dir in BlockDirection::horizontal() {
            if can_place_ladder_at(
                args.block_accessor,
                args.position,
                dir.to_block_direction(),
                args.block,
            ) {
                return true;
            }
        }
        false
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let Some(props) = ladder_properties(args.block, args.state_id) else {
            return args.state_id;
        };
        let support = props.facing.to_block_direction().opposite();
        if (support == args.direction || args.block != &Block::LADDER)
            && !can_place_ladder_at(args.world, args.position, support, args.block)
        {
            return BlockStateId::AIR;
        }
        if props.waterlogged {
            args.world.schedule_fluid_tick(
                &Fluid::WATER,
                *args.position,
                Fluid::WATER.flow_speed as u8,
                TickPriority::Normal,
            );
        }
        args.state_id
    }
}

#[must_use]
pub const fn horizontal_facing_from_offset(offset: Vector3<i32>) -> Option<BlockDirection> {
    match (offset.x, offset.y, offset.z) {
        (0, 0, -1) => Some(BlockDirection::North),
        (0, 0, 1) => Some(BlockDirection::South),
        (-1, 0, 0) => Some(BlockDirection::West),
        (1, 0, 0) => Some(BlockDirection::East),
        _ => None,
    }
}

fn can_place_ladder_at(
    block_accessor: &dyn BlockAccessor,
    block_pos: &BlockPos,
    support: BlockDirection,
    block: &Block,
) -> bool {
    if block_accessor
        .get_block_state(&block_pos.offset(support.to_offset()))
        .is_side_solid(support.opposite())
    {
        return true;
    }
    if block == &Block::LADDER {
        return false;
    }
    let (above, state) = block_accessor.get_block_and_state(&block_pos.up());
    above == block
        && ladder_properties(above, state.id)
            .is_some_and(|props| props.facing.to_block_direction() == support.opposite())
}

/// Read the facing and waterlogging from a vanilla or registered ladder state.
#[must_use]
pub fn ladder_properties(block: &Block, state: BlockStateId) -> Option<LadderLikeProperties> {
    if block == &Block::LADDER {
        return Some(LadderLikeProperties::from_state_id(state));
    }
    let raw = block.properties(state)?.to_props();
    let props = LadderLikeProperties::from_props(&raw, &Block::LADDER);
    (raw == props.to_props()).then_some(props)
}
