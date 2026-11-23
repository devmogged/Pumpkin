use std::sync::Arc;

use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId, Mirror, Rotation,
    structures::{Structure, StructureKeys},
};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::{
    math::{block_box::BlockBox, position::BlockPos, vector3::Vector3},
    random::{RandomDeriverImpl, RandomGenerator, RandomImpl, legacy_rand::LegacyRand},
};

use crate::{
    ProtoChunk,
    generation::{
        positions::chunk_pos::{start_block_x, start_block_z},
        proto_chunk::GenerationCache,
        structure::{
            piece::StructurePieceType,
            structures::{
                StructureGenerator, StructureGeneratorContext, StructurePiece, StructurePieceBase,
                StructurePiecesCollector, StructurePosition, WorldPortalExt,
            },
            template::{
                BlockPlacer, StructurePlaceSettings, StructureTemplate, get_template,
                place_template,
                processor::{IgnoredBlock, StructureProcessor},
            },
        },
    },
};

pub const FOSSILS: [&str; 14] = [
    "nether_fossils/fossil_1",
    "nether_fossils/fossil_2",
    "nether_fossils/fossil_3",
    "nether_fossils/fossil_4",
    "nether_fossils/fossil_5",
    "nether_fossils/fossil_6",
    "nether_fossils/fossil_7",
    "nether_fossils/fossil_8",
    "nether_fossils/fossil_9",
    "nether_fossils/fossil_10",
    "nether_fossils/fossil_11",
    "nether_fossils/fossil_12",
    "nether_fossils/fossil_13",
    "nether_fossils/fossil_14",
];

pub struct NetherFossilGenerator;

impl StructureGenerator for NetherFossilGenerator {
    fn get_structure_position(
        &self,
        mut context: StructureGeneratorContext<'_>,
    ) -> Option<StructurePosition> {
        // Vanilla random call order (NetherFossilStructure.java):
        // 1. nextInt(16) for X offset within chunk
        // 2. nextInt(16) for Z offset within chunk
        // 3. height.sample(random, generationContext) for initial Y (uniform 32..125)
        // 4. Column scan downward to seaLevel (no random calls)
        //    If y <= seaLevel: return empty
        // 5. Rotation.getRandom(random) - nextInt(4)
        // 6. Util.getRandom(FOSSILS, random) - nextInt(14)

        let x = start_block_x(context.chunk_x) + context.random.next_bounded_i32(16);
        let z = start_block_z(context.chunk_z) + context.random.next_bounded_i32(16);

        let structure =
            Structure::get(&context.structure_key.unwrap_or(StructureKeys::NetherFossil));
        let initial_y =
            structure
                .start_height?
                .get(&mut context.random, context.min_y as i8, context.height);

        let mut y = initial_y;
        if let Some(sampler) = context.height_sampler.as_deref_mut() {
            let mut checked_column = false;
            while y > context.sea_level {
                let Some(current) = sampler.sample_column_block(x, z, y) else {
                    break;
                };
                checked_column = true;
                y -= 1;
                let below = sampler
                    .sample_column_block(x, z, y)
                    .unwrap_or(Block::AIR.default_state);
                if current.is_air()
                    && (Block::from_state_id(below.id) == &Block::SOUL_SAND
                        || below.is_side_solid(BlockDirection::Up))
                {
                    break;
                }
            }

            if checked_column && y <= context.sea_level {
                return None;
            }
        }

        let rotation_index = context.random.next_bounded_i32(4) as u8;
        let rotation = Rotation::from_index(rotation_index);

        let template_index = context.random.next_bounded_i32(FOSSILS.len() as i32) as usize;
        let template_name = FOSSILS[template_index];

        let template = get_template(template_name)?;
        let position = Vector3::new(x, y, z);

        let mut collector = StructurePiecesCollector::default();

        let piece = NetherFossilPiece::new(template, template_name.to_string(), position, rotation);

        collector.add_piece(Box::new(piece));

        Some(StructurePosition {
            start_pos: BlockPos::new(x, y, z),
            collector: Arc::new(collector.into()),
        })
    }
}

pub struct NetherFossilPiece {
    pub piece: StructurePiece,
    pub template: Arc<StructureTemplate>,
    pub template_name: String,
    pub place_settings: StructurePlaceSettings,
    pub template_position: Vector3<i32>,
}

impl NetherFossilPiece {
    #[must_use]
    pub fn new(
        template: Arc<StructureTemplate>,
        template_name: String,
        template_position: Vector3<i32>,
        rotation: Rotation,
    ) -> Self {
        let place_settings = make_settings(rotation);
        let bounding_box = template.get_bounding_box(&place_settings, template_position);

        Self {
            piece: StructurePiece::new(StructurePieceType::NetherFossil, bounding_box, 0),
            template,
            template_name,
            place_settings,
            template_position,
        }
    }

    fn place_fossil(&self, cache: &mut dyn GenerationCache, seed: i64, chunk_box: &BlockBox) {
        let fossil_box = self.piece.bounding_box;
        let mut enlarged_box = *chunk_box;
        enlarged_box.encompass(&fossil_box);
        place_template(
            &mut FossilPlacer(cache),
            &self.template,
            // place_template normalizes rotated coordinates to the minimum corner.
            fossil_box.min,
            (0, 0),
            self.place_settings.get_rotation(),
            true,
            false,
            self.place_settings.get_processors(),
            Some(&enlarged_box),
        );
        Self::place_dried_ghast(cache, seed, &fossil_box, &enlarged_box);
    }

    fn place_dried_ghast(
        cache: &mut dyn GenerationCache,
        seed: i64,
        fossil_box: &BlockBox,
        chunk_box: &BlockBox,
    ) {
        let size = fossil_box.max - fossil_box.min + Vector3::new(1, 1, 1);
        let center = fossil_box.min + Vector3::new(size.x / 2, size.y / 2, size.z / 2);
        let splitter = LegacyRand::from_seed(seed as u64).next_splitter();
        let mut random = splitter.split_pos(center.x, center.y, center.z);

        if random.next_f32() < 0.5 {
            let pos = Vector3::new(
                fossil_box.min.x + random.next_bounded_i32(size.x),
                fossil_box.min.y,
                fossil_box.min.z + random.next_bounded_i32(size.z),
            );
            if cache.is_air(&pos) && chunk_box.contains_pos(&pos) {
                let rotation = Rotation::from_index(random.next_bounded_i32(4) as u8);
                cache.set_block_state(&pos, Block::DRIED_GHAST.default_state.rotate(rotation));
            }
        }
    }
}

impl StructurePieceBase for NetherFossilPiece {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn get_structure_piece(&self) -> &StructurePiece {
        &self.piece
    }
    fn get_structure_piece_mut(&mut self) -> &mut StructurePiece {
        &mut self.piece
    }
    fn place(
        &mut self,
        chunk: &mut ProtoChunk,
        _block_registry: &dyn WorldPortalExt,
        _random: &mut RandomGenerator,
        seed: i64,
        chunk_box: &BlockBox,
    ) {
        self.place_fossil(chunk, seed, chunk_box);
    }

    fn place_in_cache(
        &mut self,
        cache: &mut dyn GenerationCache,
        _block_registry: &dyn WorldPortalExt,
        _random: &mut RandomGenerator,
        seed: i64,
        chunk_box: &BlockBox,
    ) {
        self.place_fossil(cache, seed, chunk_box);
    }
}

struct FossilPlacer<'a>(&'a mut dyn GenerationCache);

impl BlockPlacer for FossilPlacer<'_> {
    fn get_block_state(&self, pos: &Vector3<i32>) -> BlockStateId {
        GenerationCache::get_block_state(self.0, pos)
    }

    fn set_block_state(&mut self, pos: &Vector3<i32>, state: &BlockState) {
        self.0.set_block_state(pos, state);
    }

    fn add_block_entity(&mut self, nbt: NbtCompound) {
        if let (Some(x), Some(y), Some(z)) = (nbt.get_int("x"), nbt.get_int("y"), nbt.get_int("z"))
        {
            self.0.add_block_entity(&Vector3::new(x, y, z), nbt);
        }
    }
}

fn make_settings(rotation: Rotation) -> StructurePlaceSettings {
    StructurePlaceSettings::new()
        .set_rotation(rotation)
        .set_mirror(Mirror::None)
        .add_processor(StructureProcessor::BlockIgnore(vec![
            IgnoredBlock {
                block_id: Block::STRUCTURE_BLOCK.id,
                properties: None,
            },
            IgnoredBlock {
                block_id: Block::AIR.id,
                properties: None,
            },
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::structure::structures::HeightSampler;
    use pumpkin_data::Block;

    struct MockColumnSampler {
        ground_y: i32,
    }

    impl HeightSampler for MockColumnSampler {
        fn estimate_height(&mut self, _block_x: i32, _block_z: i32) -> i32 {
            self.ground_y
        }

        fn sample_column_block(
            &mut self,
            _block_x: i32,
            _block_z: i32,
            y: i32,
        ) -> Option<&'static BlockState> {
            if y > self.ground_y {
                Some(Block::AIR.default_state)
            } else {
                Some(Block::NETHERRACK.default_state)
            }
        }
    }

    #[test]
    fn nether_fossil_writes_its_template_across_the_chunk_border() {
        use crate::chunk_system::{Cache, Chunk};
        use crate::generation::{
            get_world_gen,
            proto_chunk_test::stage_parity::TestBlockRegistry,
            structure::template::{PaletteEntry, TemplateBlock},
        };
        use pumpkin_data::dimension::Dimension;
        use pumpkin_util::world_seed::Seed;

        let generator = get_world_gen(
            Seed(42),
            Dimension::THE_NETHER,
            false,
            Vec::new(),
            String::new(),
        );
        let mut cache = Cache::new(-1, -1, 3);
        for x in -1..=1 {
            for z in -1..=1 {
                cache
                    .chunks
                    .push(Chunk::Proto(Box::new(ProtoChunk::new(x, z, &generator))));
            }
        }

        let mut template = StructureTemplate::default();
        template.size = Vector3::new(1, 1, 2);
        template
            .palette
            .push(PaletteEntry::new("minecraft:bone_block".into()));
        template.blocks.push(TemplateBlock {
            pos: Vector3::new(0, 0, 0),
            state: 0,
            nbt: None,
        });
        let mut collector = StructurePiecesCollector::default();
        collector.add_piece(Box::new(NetherFossilPiece::new(
            Arc::new(template),
            "border_fixture".into(),
            Vector3::new(16, 64, 8),
            Rotation::Clockwise90,
        )));
        collector.generate_in_chunk(
            &mut cache,
            &TestBlockRegistry,
            &mut crate::generation::structure::structures::create_chunk_random(42, 0, 0),
            42,
        );

        assert_eq!(
            GenerationCache::get_block_state(&cache, &Vector3::new(16, 64, 8)).to_block_id(),
            Block::BONE_BLOCK.id
        );
        assert_ne!(
            GenerationCache::get_block_state(&cache, &Vector3::new(17, 64, 8)).to_block_id(),
            Block::BONE_BLOCK.id
        );
    }

    #[test]
    fn nether_fossil_never_places_on_roof() {
        for seed in 0..100 {
            let mut sampler = MockColumnSampler { ground_y: 60 };
            let context = StructureGeneratorContext {
                seed,
                chunk_x: 0,
                chunk_z: 0,
                random: crate::generation::structure::structures::create_chunk_random(seed, 0, 0),
                sea_level: 32,
                min_y: 0,
                height: 128,
                height_sampler: Some(&mut sampler),
                structure_key: Some(pumpkin_data::structures::StructureKeys::NetherFossil),
            };
            if let Some(pos) = NetherFossilGenerator.get_structure_position(context) {
                assert!(
                    pos.start_pos.0.y <= 125,
                    "Fossil placed at {}, above logical nether height!",
                    pos.start_pos.0.y
                );
                assert!(
                    pos.start_pos.0.y >= 32,
                    "Fossil placed at {}, below sea level!",
                    pos.start_pos.0.y
                );
            }
        }
    }

    #[test]
    fn nether_fossil_rejects_when_no_ground_above_sea_level() {
        let mut sampler = MockColumnSampler { ground_y: 20 }; // below sea_level (32)
        let context = StructureGeneratorContext {
            seed: 42,
            chunk_x: 0,
            chunk_z: 0,
            random: crate::generation::structure::structures::create_chunk_random(42, 0, 0),
            sea_level: 32,
            min_y: 0,
            height: 128,
            height_sampler: Some(&mut sampler),
            structure_key: Some(pumpkin_data::structures::StructureKeys::NetherFossil),
        };
        assert!(
            NetherFossilGenerator
                .get_structure_position(context)
                .is_none()
        );
    }
}
