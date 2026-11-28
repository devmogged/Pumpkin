use crate::block::blocks::weathering_copper::change_over_time;
use crate::block::{
    BlockBehaviour, GetStateForNeighborUpdateArgs, OnPlaceArgs, PathComputationType,
    RandomTickArgs, is_exception_for_connection,
};
use pumpkin_data::block_properties::BlockProperties;
use pumpkin_data::fluid::Fluid;
use pumpkin_data::tag::Taggable;
use pumpkin_data::{Block, BlockDirection, BlockState, BlockStateId, HorizontalFacingExt, tag};
use pumpkin_macros::pumpkin_block_from_tag;
use pumpkin_world::tick::TickPriority;

type BarsProperties = pumpkin_data::block_properties::OakFenceLikeProperties;

#[pumpkin_block_from_tag("minecraft:bars")]
pub struct IronBarsBlock;

impl BlockBehaviour for IronBarsBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let default = args.block.default_state.id;
        let Some(mut props) = bars_properties(args.block, default) else {
            return default;
        };
        props.waterlogged = args.replacing.water_source();
        for direction in BlockDirection::horizontal() {
            let position = args.position.offset(direction.to_offset());
            let (neighbor, state) = args.world.get_block_and_state(&position);
            let face = direction.to_block_direction();
            set_side(
                &mut props,
                face,
                attaches_to(args.block, state, neighbor, face),
            );
        }
        bars_state(args.block, &props)
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let Some(mut props) = bars_properties(args.block, args.state_id) else {
            return args.state_id;
        };
        if props.waterlogged {
            args.world.schedule_fluid_tick(
                &Fluid::WATER,
                *args.position,
                pumpkin_data::fluid::Fluid::WATER.flow.tick_delay,
                TickPriority::Normal,
            );
        }
        if !args.direction.is_horizontal() {
            return args.state_id;
        }
        set_side(
            &mut props,
            args.direction,
            attaches_to(
                args.block,
                args.neighbor_state_id.to_state(),
                args.neighbor_state_id.to_block(),
                args.direction,
            ),
        );
        bars_state(args.block, &props)
    }

    fn random_tick(&self, args: RandomTickArgs<'_>) {
        change_over_time(args.world, args.position, args.block);
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}

fn attaches_to(block: &Block, state: &BlockState, neighbor: &Block, side: BlockDirection) -> bool {
    !is_exception_for_connection(neighbor) && state.is_side_solid(side.opposite())
        || neighbor == block
        || neighbor.has_tag(&tag::Block::MINECRAFT_BARS)
        || neighbor.has_tag(&tag::Block::C_GLASS_PANES)
        || neighbor.has_tag(&tag::Block::MINECRAFT_WALLS)
}

fn set_side(props: &mut BarsProperties, face: BlockDirection, connected: bool) {
    match face {
        BlockDirection::North => props.north = connected,
        BlockDirection::East => props.east = connected,
        BlockDirection::South => props.south = connected,
        BlockDirection::West => props.west = connected,
        BlockDirection::Up | BlockDirection::Down => {}
    }
}

fn bars_properties(block: &Block, state: BlockStateId) -> Option<BarsProperties> {
    if BarsProperties::handles_block_id(block.id) {
        return Some(BarsProperties::from_state_id(state));
    }
    let raw = block.properties(state)?.to_props();
    let decoded = BarsProperties::from_props(&raw, &Block::IRON_BARS);
    (decoded.to_props() == raw).then_some(decoded)
}

fn bars_state(block: &Block, props: &BarsProperties) -> BlockStateId {
    if BarsProperties::handles_block_id(block.id) {
        return props.to_state_id(block);
    }
    block.from_properties(&props.to_props()).to_state_id(block)
}
