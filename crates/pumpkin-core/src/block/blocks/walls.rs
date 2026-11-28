use crate::block::{
    BlockBehaviour, GetStateForNeighborUpdateArgs, OnPlaceArgs, PathComputationType,
    is_exception_for_connection,
};
use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId,
    block_properties::{
        BlockProperties, EastWall, HorizontalFacing, NorthWall, SouthWall, WestWall,
    },
    fluid::Fluid,
    tag::{self, Taggable},
};
use pumpkin_macros::pumpkin_block_from_tag;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::tick::TickPriority;

type FenceGateProperties = pumpkin_data::block_properties::OakFenceGateLikeProperties;
type WallProperties = pumpkin_data::block_properties::ResinBrickWallLikeProperties;

const DIRECTIONS: [HorizontalFacing; 4] = [
    HorizontalFacing::North,
    HorizontalFacing::East,
    HorizontalFacing::South,
    HorizontalFacing::West,
];

type Rectangle = [f64; 4];

const POST_TEST: Rectangle = [7. / 16., 7. / 16., 9. / 16., 9. / 16.];
const WALL_TESTS: [Rectangle; 4] = [
    [7. / 16., 0., 9. / 16., 9. / 16.],
    [7. / 16., 7. / 16., 1., 9. / 16.],
    [7. / 16., 7. / 16., 9. / 16., 1.],
    [0., 7. / 16., 9. / 16., 9. / 16.],
];

#[pumpkin_block_from_tag("minecraft:walls")]
pub struct WallBlock;

impl BlockBehaviour for WallBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let default = args.block.default_state.id;
        let Some(mut props) = wall_properties(args.block, default) else {
            return default;
        };
        props.waterlogged = args.replacing.water_source();
        let connections = DIRECTIONS.map(|direction| {
            let position = args.position.offset(direction.to_offset());
            let (block, state) = args.world.get_block_and_state(&position);
            connects_to(direction, block, state)
        });
        let above = args.position.up();
        let top_state = args.world.get_block_state(&above);
        update_shape(props, args.block, connections, top_state, &above)
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let Some(props) = wall_properties(args.block, args.state_id) else {
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
        let side = match args.direction {
            BlockDirection::Down => return args.state_id,
            BlockDirection::Up => None,
            BlockDirection::North => Some(0),
            BlockDirection::East => Some(1),
            BlockDirection::South => Some(2),
            BlockDirection::West => Some(3),
        };
        let neighbor_state = args.neighbor_state_id.to_state();
        let mut connections = [
            props.north != NorthWall::None,
            props.east != EastWall::None,
            props.south != SouthWall::None,
            props.west != WestWall::None,
        ];
        let above = args.position.up();
        let top_state = if let Some(index) = side {
            connections[index] = connects_to(
                DIRECTIONS[index],
                args.neighbor_state_id.to_block(),
                neighbor_state,
            );
            args.world.get_block_state(&above)
        } else {
            neighbor_state
        };
        update_shape(props, args.block, connections, top_state, &above)
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}

fn update_shape(
    mut props: WallProperties,
    block: &Block,
    connections: [bool; 4],
    above_state: &BlockState,
    above_pos: &BlockPos,
) -> BlockStateId {
    let above_block = above_state.id.to_block();
    let above_face: Vec<_> = above_state
        .get_block_collision_shapes_at(above_pos)
        .filter(|shape| shape.min.y <= 0. && shape.max.y > 0.)
        .map(|shape| [shape.min.x, shape.min.z, shape.max.x, shape.max.z])
        .collect();
    let [north, east, south, west] =
        std::array::from_fn(|i| make_wall_state(connections[i], &above_face, WALL_TESTS[i]));
    props.north = north.into();
    props.east = east.into();
    props.south = south.into();
    props.west = west.into();
    let [connect_north, connect_east, connect_south, connect_west] = connections;
    let has_corner = !connections.into_iter().any(|connected| connected)
        || connect_north != connect_south
        || connect_east != connect_west;
    let has_high_wall = north == WallShape::Tall && south == WallShape::Tall
        || east == WallShape::Tall && west == WallShape::Tall;
    let top_has_post = above_block.has_tag(&tag::Block::MINECRAFT_WALLS)
        && wall_properties(above_block, above_state.id).is_some_and(|props| props.up);
    props.up = top_has_post
        || has_corner
        || !has_high_wall
            && (above_block.has_tag(&tag::Block::MINECRAFT_WALL_POST_OVERRIDE)
                || is_covered(&above_face, POST_TEST));

    if WallProperties::handles_block_id(block.id) {
        props.to_state_id(block)
    } else {
        block.from_properties(&props.to_props()).to_state_id(block)
    }
}

fn make_wall_state(connected: bool, above: &[Rectangle], test: Rectangle) -> WallShape {
    if !connected {
        WallShape::None
    } else if is_covered(above, test) {
        WallShape::Tall
    } else {
        WallShape::Low
    }
}

fn is_covered(rectangles: &[Rectangle], [mut x, z_min, x_max, z_max]: Rectangle) -> bool {
    // Each strip starts or ends at a rectangle boundary. Coverage within it varies only along Z.
    while x < x_max {
        let next_x = rectangles
            .iter()
            .flat_map(|[min_x, _, max_x, _]| [*min_x, *max_x])
            .filter(|&edge| edge > x)
            .fold(x_max, f64::min);
        let mut z = z_min;
        while z < z_max {
            let next_z = rectangles
                .iter()
                .filter(|&[min_x, min_z, max_x, _]| *min_x <= x && *max_x >= next_x && *min_z <= z)
                .map(|rectangle| rectangle[3])
                .fold(z, f64::max);
            if next_z == z {
                return false;
            }
            z = next_z;
        }
        x = next_x;
    }
    true
}

fn wall_properties(block: &Block, state: BlockStateId) -> Option<WallProperties> {
    if WallProperties::handles_block_id(block.id) {
        return Some(WallProperties::from_state_id(state));
    }
    let props = block.properties(state)?.to_props();
    // The raw selector "none" is also used by redstone wire.
    let decoded = WallProperties::from_props(&props, &Block::COBBLESTONE_WALL);
    let wall = decoded.to_props();
    (wall.len() == props.len()
        && wall
            .iter()
            .zip(&props)
            .all(|(wall, raw)| wall.matches_property(*raw)))
    .then_some(decoded)
}

fn connects_to(direction: HorizontalFacing, block: &Block, state: &BlockState) -> bool {
    block.has_tag(&tag::Block::MINECRAFT_WALLS)
        || !is_exception_for_connection(block)
            && state.is_side_solid(BlockDirection::from_cardinal_direction(
                direction.opposite(),
            ))
        || block.has_tag(&tag::Block::MINECRAFT_BARS)
        || block.has_tag(&tag::Block::C_GLASS_PANES)
        || FenceGateProperties::handles_block_id(block.id) && {
            let gate = FenceGateProperties::from_state_id(state.id);
            gate.facing == direction.rotate_clockwise()
                || gate.facing == direction.rotate_counter_clockwise()
        }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WallShape {
    None,
    Low,
    Tall,
}

macro_rules! convert_shape {
    ($enum:ident) => {
        impl From<WallShape> for $enum {
            fn from(value: WallShape) -> Self {
                match value {
                    WallShape::None => Self::None,
                    WallShape::Low => Self::Low,
                    WallShape::Tall => Self::Tall,
                }
            }
        }
    };
}

convert_shape!(NorthWall);
convert_shape!(EastWall);
convert_shape!(SouthWall);
convert_shape!(WestWall);

#[cfg(test)]
mod tests {
    use super::{POST_TEST, WALL_TESTS, is_covered};

    #[test]
    fn unions_of_bottom_faces_can_cover_post_and_wall_probes() {
        let two_halves = [[0., 0., 0.5, 1.], [0.5, 0., 1., 1.]];
        assert!(is_covered(&two_halves, POST_TEST));
        assert!(is_covered(&two_halves, WALL_TESTS[0]));
        let gap = [[0., 0., 0.49, 1.], [0.51, 0., 1., 1.]];
        assert!(!is_covered(&gap, POST_TEST));
    }
}
