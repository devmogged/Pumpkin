use crate::block::{
    BlockBehaviour, GetStateForNeighborUpdateArgs, OnPlaceArgs, PathComputationType,
    blocks::iron_bars::IronBarsBlock,
};
use pumpkin_data::{BlockState, BlockStateId};
use pumpkin_macros::pumpkin_block_from_tag;

#[pumpkin_block_from_tag("c:glass_panes")]
pub struct GlassPaneBlock;

impl BlockBehaviour for GlassPaneBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        IronBarsBlock.on_place(args)
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        IronBarsBlock.get_state_for_neighbor_update(args)
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}
