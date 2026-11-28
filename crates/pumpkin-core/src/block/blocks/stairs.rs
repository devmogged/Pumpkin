use crate::entity::EntityBase;
use pumpkin_data::block_properties::Half;
use pumpkin_data::block_properties::HorizontalFacing;
use pumpkin_data::block_properties::StairsShape;
use pumpkin_data::block_properties::{BlockProperties, Facing};
use pumpkin_data::fluid::Fluid;
use pumpkin_data::tag::Taggable;
use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId, Mirror, Property, Rotation, tag,
};
use pumpkin_macros::pumpkin_block_from_tag;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::tick::TickPriority;

use crate::block::{
    BlockBehaviour, GetStateForNeighborUpdateArgs, OnPlaceArgs, PathComputationType,
};
use crate::world::World;

type StairsProperties = pumpkin_data::block_properties::WhiteWoolStairsLikeProperties;

#[pumpkin_block_from_tag("minecraft:stairs")]
pub struct StairBlock;

impl BlockBehaviour for StairBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let Some(mut stair_props) = stair_properties(args.block, args.block.default_state.id)
        else {
            return args.block.default_state.id;
        };
        stair_props.waterlogged = args.replacing.water_source();

        stair_props.facing = args.player.get_entity().get_horizontal_facing();
        stair_props.half = match args.direction {
            BlockDirection::Up => Half::Top,
            BlockDirection::Down => Half::Bottom,
            _ => match args.use_item_on.cursor_pos.y {
                0.0..=0.5 => Half::Bottom,
                _ => Half::Top,
            },
        };

        stair_props.shape = compute_stair_shape(
            args.world,
            args.position,
            stair_props.facing,
            stair_props.half,
        );

        stair_state(args.block, &stair_props)
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let Some(mut stair_props) = stair_properties(args.block, args.state_id) else {
            return args.state_id;
        };
        if stair_props.waterlogged {
            args.world.schedule_fluid_tick(
                &Fluid::WATER,
                *args.position,
                pumpkin_data::fluid::Fluid::WATER.flow.tick_delay,
                TickPriority::Normal,
            );
        }

        if args.direction.is_horizontal() {
            stair_props.shape = compute_stair_shape(
                args.world,
                args.position,
                stair_props.facing,
                stair_props.half,
            );
            return stair_state(args.block, &stair_props);
        }

        args.state_id
    }

    fn rotate(
        &self,
        block: &Block,
        state_id: BlockStateId,
        rotation: Rotation,
    ) -> &'static BlockState {
        let Some(mut stair_props) = stair_properties(block, state_id) else {
            return BlockState::from_id(state_id);
        };
        stair_props.facing = rotation.rotate_horizontal(stair_props.facing);
        BlockState::from_id(stair_state(block, &stair_props))
    }

    fn mirror(&self, block: &Block, state_id: BlockStateId, mirror: Mirror) -> &'static BlockState {
        let Some(mut stair_props) = stair_properties(block, state_id) else {
            return BlockState::from_id(state_id);
        };
        let direction = stair_props.facing;
        let shape = stair_props.shape;

        match mirror {
            Mirror::LeftRight => {
                if matches!(direction, HorizontalFacing::North | HorizontalFacing::South) {
                    stair_props.facing = stair_props.facing.opposite();
                    stair_props.shape = match shape {
                        StairsShape::Straight => StairsShape::Straight,
                        StairsShape::OuterLeft => StairsShape::OuterRight,
                        StairsShape::InnerRight => StairsShape::InnerLeft,
                        StairsShape::InnerLeft => StairsShape::InnerRight,
                        StairsShape::OuterRight => StairsShape::OuterLeft,
                    };
                    return BlockState::from_id(stair_state(block, &stair_props));
                }
            }
            Mirror::FrontBack => {
                if matches!(direction, HorizontalFacing::East | HorizontalFacing::West) {
                    stair_props.facing = stair_props.facing.opposite();
                    stair_props.shape = match shape {
                        StairsShape::Straight => StairsShape::Straight,
                        StairsShape::OuterLeft => StairsShape::OuterRight,
                        StairsShape::InnerRight => StairsShape::InnerRight,
                        StairsShape::InnerLeft => StairsShape::InnerLeft,
                        StairsShape::OuterRight => StairsShape::OuterLeft,
                    };
                    return BlockState::from_id(stair_state(block, &stair_props));
                }
            }
            Mirror::None => {}
        }

        BlockState::from_id(state_id)
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}

fn compute_stair_shape(
    world: &World,
    block_pos: &BlockPos,
    facing: HorizontalFacing,
    half: Half,
) -> StairsShape {
    let right_locked = get_stair_properties_if_exists(
        world,
        &block_pos.offset(facing.rotate_clockwise().to_offset()),
    )
    .is_some_and(|other_stair_props| {
        other_stair_props.half == half && other_stair_props.facing == facing
    });

    let left_locked = get_stair_properties_if_exists(
        world,
        &block_pos.offset(facing.rotate_counter_clockwise().to_offset()),
    )
    .is_some_and(|other_stair_props| {
        other_stair_props.half == half && other_stair_props.facing == facing
    });

    if left_locked && right_locked {
        return StairsShape::Straight;
    }

    if let Some(other_stair_props) =
        get_stair_properties_if_exists(world, &block_pos.offset(facing.to_offset()))
        && other_stair_props.half == half
    {
        if !left_locked && other_stair_props.facing == facing.rotate_clockwise() {
            return StairsShape::OuterRight;
        } else if !right_locked && other_stair_props.facing == facing.rotate_counter_clockwise() {
            return StairsShape::OuterLeft;
        }
    }

    if let Some(other_stair_props) =
        get_stair_properties_if_exists(world, &block_pos.offset(facing.opposite().to_offset()))
        && other_stair_props.half == half
    {
        if !right_locked && other_stair_props.facing == facing.rotate_clockwise() {
            return StairsShape::InnerRight;
        } else if !left_locked && other_stair_props.facing == facing.rotate_counter_clockwise() {
            return StairsShape::InnerLeft;
        }
    }

    StairsShape::Straight
}

fn get_stair_properties_if_exists(world: &World, block_pos: &BlockPos) -> Option<StairsProperties> {
    let (block, block_state) = world.get_block_and_state_id(block_pos);
    if !block.has_tag(&tag::Block::MINECRAFT_STAIRS) {
        return None;
    }
    stair_properties(block, block_state)
}

fn stair_properties(block: &Block, state: BlockStateId) -> Option<StairsProperties> {
    if StairsProperties::handles_block_id(block.id) {
        return Some(StairsProperties::from_state_id(state));
    }
    let props = block.properties(state)?.to_props();
    let [
        Property::Facing(facing),
        Property::Half(half),
        Property::StairsShape(shape),
        Property::Waterlogged(waterlogged),
    ] = props.as_slice()
    else {
        return None;
    };
    let facing = match facing {
        Facing::North => HorizontalFacing::North,
        Facing::East => HorizontalFacing::East,
        Facing::South => HorizontalFacing::South,
        Facing::West => HorizontalFacing::West,
        Facing::Up | Facing::Down => return None,
    };
    Some(StairsProperties {
        facing,
        half: *half,
        shape: *shape,
        waterlogged: *waterlogged,
    })
}

fn stair_state(block: &Block, props: &StairsProperties) -> BlockStateId {
    if StairsProperties::handles_block_id(block.id) {
        return props.to_state_id(block);
    }
    block.from_properties(&props.to_props()).to_state_id(block)
}
