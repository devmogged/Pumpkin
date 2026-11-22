use pumpkin_data::{Block, BlockDirection, BlockId, tag};
use pumpkin_util::{
    math::{float_provider::FloatProvider, int_provider::IntProvider, position::BlockPos},
    random::{RandomGenerator, RandomImpl},
};

use std::collections::HashSet;

use super::speleothem::{
    Column, grow_speleothem, is_empty_or_water, is_neither_empty_nor_water, is_water,
    place_base_block_if_possible, state_at,
};
use crate::generation::{
    feature::features::spring_feature::BlockWrapper, proto_chunk::GenerationCache,
};

/// Java's `SpeleothemClusterFeature` (formerly the dripstone cluster).
pub struct DripstoneClusterFeature {
    pub base_block: &'static Block,
    pub pointed_block: &'static Block,
    pub replaceable_blocks: BlockWrapper,
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub speleothem_block_layer_thickness: IntProvider,
    pub density: FloatProvider,
    pub wetness: FloatProvider,
    pub chance_of_speleothem_at_max_distance_from_center: f32,
    pub max_distance_from_edge_affecting_chance_of_speleothem: i32,
    pub max_distance_from_center_affecting_height_bias: i32,
}

/// `Mth.clampedMap` with float arguments.
fn clamped_map_f32(value: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    let factor = (value - from_min) / (from_max - from_min);
    if factor < 0.0 {
        to_min
    } else if factor > 1.0 {
        to_max
    } else {
        to_min + factor * (to_max - to_min)
    }
}

/// `Mth.clampedMap` with double arguments.
fn clamped_map_f64(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    let factor = (value - from_min) / (from_max - from_min);
    if factor < 0.0 {
        to_min
    } else if factor > 1.0 {
        to_max
    } else {
        to_min + factor * (to_max - to_min)
    }
}

/// `Mth.randomBetweenInclusive`.
fn random_between_inclusive(random: &mut RandomGenerator, min: i32, max_inclusive: i32) -> i32 {
    random.next_bounded_i32(max_inclusive - min + 1) + min
}

impl DripstoneClusterFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        random: &mut RandomGenerator,
        origin: BlockPos,
    ) -> bool {
        if !is_empty_or_water(state_at(chunk, origin)) {
            return false;
        }
        let replaceable = self.replaceable_blocks.block_ids();
        let height = self.height.get(random);
        let wetness = self.wetness.get(random);
        let density = self.density.get(random);
        let x_radius = self.radius.get(random);
        let z_radius = self.radius.get(random);
        for dx in -x_radius..=x_radius {
            for dz in -z_radius..=z_radius {
                let chance = self.chance_of_speleothem(x_radius, z_radius, dx, dz);
                let pos = origin.add(dx, 0, dz);
                self.place_column(
                    chunk,
                    random,
                    pos,
                    dx,
                    dz,
                    wetness,
                    chance,
                    height,
                    density,
                    &replaceable,
                );
            }
        }
        true
    }

    #[expect(clippy::too_many_arguments)]
    fn place_column<T: GenerationCache>(
        &self,
        chunk: &mut T,
        random: &mut RandomGenerator,
        pos: BlockPos,
        dx: i32,
        dz: i32,
        chance_of_water: f32,
        chance: f64,
        cluster_height: i32,
        density: f32,
        replaceable: &HashSet<BlockId>,
    ) {
        let Some(base_column) = Column::scan(
            chunk,
            pos,
            self.floor_to_ceiling_search_range,
            is_empty_or_water,
            is_neither_empty_nor_water,
        ) else {
            return;
        };
        let ceiling = base_column.ceiling;
        let base_floor = base_column.floor;
        if ceiling.is_none() && base_floor.is_none() {
            return;
        }
        let at = |y: i32| BlockPos::new(pos.0.x, y, pos.0.z);

        let want_pool = random.next_f32() < chance_of_water;
        let column = match base_floor {
            Some(floor_y) if want_pool && self.can_place_pool(chunk, at(floor_y)) => {
                chunk.set_block_state(&at(floor_y).0, Block::WATER.default_state);
                Column {
                    floor: Some(floor_y - 1),
                    ceiling,
                }
            }
            _ => base_column,
        };
        let floor = column.floor;

        let want_stalactite = random.next_f64() < chance;
        let stalactite_height = match ceiling {
            Some(ceiling_y) if want_stalactite && !self.is_lava(chunk, at(ceiling_y)) => {
                let thickness = self.speleothem_block_layer_thickness.get(random);
                self.replace_with_base(chunk, at(ceiling_y), thickness, 1, replaceable);
                let max_height = floor.map_or(cluster_height, |floor_y| {
                    cluster_height.min(ceiling_y - floor_y)
                });
                self.speleothem_height(random, dx, dz, density, max_height)
            }
            _ => 0,
        };

        let want_stalagmite = random.next_f64() < chance;
        let stalagmite_height = match floor {
            Some(floor_y) if want_stalagmite && !self.is_lava(chunk, at(floor_y)) => {
                let thickness = self.speleothem_block_layer_thickness.get(random);
                self.replace_with_base(chunk, at(floor_y), thickness, -1, replaceable);
                if ceiling.is_some() {
                    let diff = self.max_stalagmite_stalactite_height_diff;
                    0.max(stalactite_height + random_between_inclusive(random, -diff, diff))
                } else {
                    self.speleothem_height(random, dx, dz, density, cluster_height)
                }
            }
            _ => 0,
        };

        let (stalactite, stalagmite) = match (ceiling, floor) {
            (Some(ceiling_y), Some(floor_y))
                if ceiling_y - stalactite_height <= floor_y + stalagmite_height =>
            {
                let lowest_bottom = (ceiling_y - stalactite_height).max(floor_y + 1);
                let highest_top = (floor_y + stalagmite_height).min(ceiling_y - 1);
                let bottom = random_between_inclusive(random, lowest_bottom, highest_top + 1);
                (ceiling_y - bottom, bottom - 1 - floor_y)
            }
            _ => (stalactite_height, stalagmite_height),
        };

        let merge_tips = random.next_bool()
            && stalactite > 0
            && stalagmite > 0
            && column.height() == Some(stalactite + stalagmite);
        if let Some(ceiling_y) = ceiling {
            grow_speleothem(
                chunk,
                at(ceiling_y - 1),
                BlockDirection::Down,
                stalactite,
                merge_tips,
                self.base_block,
                self.pointed_block,
                replaceable,
            );
        }
        if let Some(floor_y) = floor {
            grow_speleothem(
                chunk,
                at(floor_y + 1),
                BlockDirection::Up,
                stalagmite,
                merge_tips,
                self.base_block,
                self.pointed_block,
                replaceable,
            );
        }
    }

    fn is_lava<T: GenerationCache>(&self, chunk: &T, pos: BlockPos) -> bool {
        let _ = self;
        state_at(chunk, pos).to_block_id() == BlockId::LAVA
    }

    fn speleothem_height(
        &self,
        random: &mut RandomGenerator,
        dx: i32,
        dz: i32,
        density: f32,
        max_height: i32,
    ) -> i32 {
        if random.next_f32() > density {
            return 0;
        }
        let distance = dx.abs() + dz.abs();
        let mean = clamped_map_f64(
            f64::from(distance),
            0.0,
            f64::from(self.max_distance_from_center_affecting_height_bias),
            f64::from(max_height) / 2.0,
            0.0,
        ) as f32;
        // ClampedNormalFloat.sample: Mth.normal, then clamp.
        let sample = mean + random.next_gaussian() as f32 * self.height_deviation as f32;
        sample.clamp(0.0, max_height as f32) as i32
    }

    fn can_place_pool<T: GenerationCache>(&self, chunk: &T, pos: BlockPos) -> bool {
        let block = state_at(chunk, pos).to_block_id();
        if block == BlockId::WATER || block == self.base_block.id || block == self.pointed_block.id
        {
            return false;
        }
        if is_water(state_at(chunk, pos.up())) {
            return false;
        }
        let adjacent = |pos: BlockPos| {
            let state = state_at(chunk, pos);
            state
                .to_block_id()
                .has_tag(tag::Block::MINECRAFT_BASE_STONE_OVERWORLD)
                || is_water(state)
        };
        BlockDirection::horizontal()
            .iter()
            .all(|direction| adjacent(pos.offset(direction.to_offset())))
            && adjacent(pos.down())
    }

    fn replace_with_base<T: GenerationCache>(
        &self,
        chunk: &mut T,
        first: BlockPos,
        max_count: i32,
        step: i32,
        replaceable: &HashSet<BlockId>,
    ) {
        let mut pos = first;
        for _ in 0..max_count {
            if !place_base_block_if_possible(chunk, pos, self.base_block, replaceable) {
                return;
            }
            pos = pos.up_height(step);
        }
    }

    fn chance_of_speleothem(&self, x_radius: i32, z_radius: i32, dx: i32, dz: i32) -> f64 {
        let x_from_edge = x_radius - dx.abs();
        let z_from_edge = z_radius - dz.abs();
        let from_edge = x_from_edge.min(z_from_edge);
        f64::from(clamped_map_f32(
            from_edge as f32,
            0.0,
            self.max_distance_from_edge_affecting_chance_of_speleothem as f32,
            self.chance_of_speleothem_at_max_distance_from_center,
            1.0,
        ))
    }
}
