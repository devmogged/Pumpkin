use pumpkin_data::{Block, BlockId, tag};
use pumpkin_util::{
    math::{
        cos, float_provider::FloatProvider, int_provider::IntProvider, position::BlockPos, sin,
    },
    random::{RandomGenerator, RandomImpl},
};

use super::speleothem::{
    Column, is_base_or_lava, is_circle_mostly_embedded_in_stone, is_empty_or_water,
    is_empty_or_water_or_lava, speleothem_height, state_at,
};
use crate::generation::{
    feature::features::spring_feature::BlockWrapper, proto_chunk::GenerationCache,
};

/// Java's `LargeDripstoneFeature`.
pub struct LargeDripstoneFeature {
    pub replaceable_blocks: BlockWrapper,
    pub floor_to_ceiling_search_range: i32,
    pub column_radius: IntProvider,
    pub height_scale: FloatProvider,
    pub max_column_radius_to_cave_height_ratio: f32,
    pub stalactite_bluntness: FloatProvider,
    pub stalagmite_bluntness: FloatProvider,
    pub wind_speed: FloatProvider,
    pub min_radius_for_wind: i32,
    pub min_bluntness_for_wind: f32,
}

/// `Mth.randomBetween`.
fn random_between(random: &mut RandomGenerator, min: f32, max: f32) -> f32 {
    random.next_f32() * (max - min) + min
}

impl LargeDripstoneFeature {
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
        let column = Column::scan(
            chunk,
            origin,
            self.floor_to_ceiling_search_range,
            is_empty_or_water,
            |state| is_base_or_lava(state, &Block::DRIPSTONE_BLOCK, &replaceable),
        );
        let Some(Column {
            floor: Some(floor),
            ceiling: Some(ceiling),
        }) = column
        else {
            return false;
        };
        let column_height = ceiling - floor - 1;
        if column_height < 4 {
            return false;
        }

        let by_height = (column_height as f32 * self.max_column_radius_to_cave_height_ratio) as i32;
        let (min_radius, max_radius) = (self.column_radius.get_min(), self.column_radius.get_max());
        let max_column_radius = by_height.clamp(min_radius, max_radius);
        let radius = random.next_bounded_i32(max_column_radius - min_radius + 1) + min_radius;
        let at = |y: i32| BlockPos::new(origin.0.x, y, origin.0.z);
        let mut stalactite = Dripstone::new(
            at(ceiling - 1),
            false,
            random,
            radius,
            &self.stalactite_bluntness,
            &self.height_scale,
        );
        let mut stalagmite = Dripstone::new(
            at(floor + 1),
            true,
            random,
            radius,
            &self.stalagmite_bluntness,
            &self.height_scale,
        );

        let wind = if stalactite.suits_wind(self.min_radius_for_wind, self.min_bluntness_for_wind)
            && stalagmite.suits_wind(self.min_radius_for_wind, self.min_bluntness_for_wind)
        {
            Wind::new(origin.0.y, random, &self.wind_speed, 16 - radius)
        } else {
            Wind::none()
        };

        let stalactite_embedded = stalactite.move_back_until_embedded(chunk, &wind);
        let stalagmite_embedded = stalagmite.move_back_until_embedded(chunk, &wind);
        if stalactite_embedded {
            stalactite.place(chunk, random, &wind);
        }
        if stalagmite_embedded {
            stalagmite.place(chunk, random, &wind);
        }
        true
    }
}

struct Dripstone {
    root: BlockPos,
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

impl Dripstone {
    fn new(
        root: BlockPos,
        pointing_up: bool,
        random: &mut RandomGenerator,
        radius: i32,
        bluntness: &FloatProvider,
        scale: &FloatProvider,
    ) -> Self {
        let bluntness = f64::from(bluntness.get(random));
        let scale = f64::from(scale.get(random));
        Self {
            root,
            pointing_up,
            radius,
            bluntness,
            scale,
        }
    }

    fn height_at(&self, radius: f32) -> i32 {
        speleothem_height(
            f64::from(radius),
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }

    fn move_back_until_embedded<T: GenerationCache>(&mut self, chunk: &T, wind: &Wind) -> bool {
        while self.radius > 1 {
            let mut root = self.root;
            let tries = 10.min(self.height_at(0.0));
            for _ in 0..tries {
                if state_at(chunk, root).to_block_id() == BlockId::LAVA {
                    return false;
                }
                if is_circle_mostly_embedded_in_stone(chunk, wind.offset(root), self.radius) {
                    self.root = root;
                    return true;
                }
                root = root.up_height(if self.pointing_up { -1 } else { 1 });
            }
            self.radius /= 2;
        }
        false
    }

    fn place<T: GenerationCache>(&self, chunk: &mut T, random: &mut RandomGenerator, wind: &Wind) {
        let step = if self.pointing_up { 1 } else { -1 };
        for dx in -self.radius..=self.radius {
            for dz in -self.radius..=self.radius {
                let current = ((dx * dx + dz * dz) as f32).sqrt();
                if current > self.radius as f32 {
                    continue;
                }
                let mut height = self.height_at(current);
                if height <= 0 {
                    continue;
                }
                if f64::from(random.next_f32()) < 0.2 {
                    height = (height as f32 * random_between(random, 0.8, 1.0)) as i32;
                }
                let mut pos = self.root.add(dx, 0, dz);
                let mut out_of_stone = false;
                let max_y = if self.pointing_up {
                    chunk.top_block_wg_height_exclusive(pos.0.x, pos.0.z)
                } else {
                    i32::MAX
                };
                let mut i = 0;
                while i < height && pos.0.y < max_y {
                    let adjusted = wind.offset(pos);
                    let state = state_at(chunk, adjusted);
                    if is_empty_or_water_or_lava(state) {
                        out_of_stone = true;
                        chunk.set_block_state(&adjusted.0, Block::DRIPSTONE_BLOCK.default_state);
                    } else if out_of_stone
                        && state
                            .to_block_id()
                            .has_tag(tag::Block::MINECRAFT_BASE_STONE_OVERWORLD)
                    {
                        break;
                    }
                    pos = pos.up_height(step);
                    i += 1;
                }
            }
        }
    }

    fn suits_wind(&self, min_radius: i32, min_bluntness: f32) -> bool {
        self.radius >= min_radius && self.bluntness >= f64::from(min_bluntness)
    }
}

struct Wind {
    origin_y: i32,
    speed: Option<(f64, f64)>,
    max_offset: i32,
}

impl Wind {
    fn new(
        origin_y: i32,
        random: &mut RandomGenerator,
        speed: &FloatProvider,
        max_offset: i32,
    ) -> Self {
        let speed = speed.get(random);
        let direction = random_between(random, 0.0, std::f32::consts::PI);
        Self {
            origin_y,
            speed: Some((
                f64::from(cos(direction) * speed),
                f64::from(sin(direction) * speed),
            )),
            max_offset,
        }
    }

    const fn none() -> Self {
        Self {
            origin_y: 0,
            speed: None,
            max_offset: 0,
        }
    }

    fn offset(&self, pos: BlockPos) -> BlockPos {
        let Some((x, z)) = self.speed else {
            return pos;
        };
        let dy = f64::from(self.origin_y - pos.0.y);
        let dx = ((x * dy).floor() as i32).clamp(-self.max_offset, self.max_offset);
        let dz = ((z * dy).floor() as i32).clamp(-self.max_offset, self.max_offset);
        pos.add(dx, 0, dz)
    }
}
