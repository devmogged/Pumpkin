#![allow(clippy::expect_used, clippy::unwrap_used, clippy::unreachable)]

//! Datapack structures and structure sets. A separate test binary, because installing the
//! structure registry changes structure generation for the whole process.

use std::{fs, path::Path};

use pumpkin_data::{
    dimension::Dimension,
    structures::{StructureKeys, StructurePlacementType, StructureSet},
};
use pumpkin_util::world_seed::Seed;
use pumpkin_world::{
    ProtoChunk,
    generation::{
        generator::WorldGenerator, get_world_gen,
        structure::placement::get_structure_chunk_in_region, worldgen_overrides::WorldgenOverrides,
    },
};

const SEED: u64 = 42;

/// A copy of the plains village in its own set, placed like vanilla villages.
fn write_pack(root: &Path) {
    let vanilla =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/datapack/data/minecraft/worldgen");
    let worldgen = root.join("data/fixture/worldgen");
    fs::create_dir_all(worldgen.join("structure")).unwrap();
    fs::create_dir_all(worldgen.join("structure_set")).unwrap();
    fs::copy(
        vanilla.join("structure/village_plains.json"),
        worldgen.join("structure/plains_copy.json"),
    )
    .unwrap();
    let villages: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(vanilla.join("structure_set/villages.json")).unwrap(),
    )
    .unwrap();
    let copies = serde_json::json!({
        "placement": villages["placement"],
        "structures": [{ "structure": "fixture:plains_copy", "weight": 1 }],
    });
    fs::write(
        worldgen.join("structure_set/copies.json"),
        copies.to_string(),
    )
    .unwrap();
}

#[test]
fn datapack_structure_starts_like_its_vanilla_original() {
    let pack = tempfile::tempdir().unwrap();
    write_pack(pack.path());
    WorldgenOverrides::from_packs([pack.path()])
        .install_structures()
        .unwrap();

    let copy = StructureKeys::from_name("fixture:plains_copy").expect("registered copy");
    assert!(matches!(copy, StructureKeys::Custom(_)));
    assert_eq!(copy.to_name(), "fixture:plains_copy");
    assert!(StructureSet::get("fixture:copies").is_some());

    let world_gen = get_world_gen(
        Seed(SEED),
        Dimension::OVERWORLD,
        false,
        Vec::new(),
        String::new(),
    );
    let WorldGenerator::Noise(generator) = &*world_gen else {
        unreachable!()
    };
    let StructurePlacementType::RandomSpread(spread) = &StructureSet::get("villages")
        .unwrap()
        .placement
        .placement_type
    else {
        unreachable!()
    };
    let salt = StructureSet::get("villages").unwrap().placement.salt;

    let mut villages = 0;
    for region_x in -3..3 {
        for region_z in -3..3 {
            let (chunk_x, chunk_z) =
                get_structure_chunk_in_region(spread, SEED as i64, region_x, region_z, salt);
            let mut chunk = ProtoChunk::new(chunk_x, chunk_z, &world_gen);
            chunk.step_to_biomes(generator);
            chunk.set_structure_starts(generator);
            let vanilla = chunk.structure_start_boxes(StructureKeys::VillagePlains);
            assert_eq!(
                chunk.structure_start_boxes(copy),
                vanilla,
                "chunk {chunk_x} {chunk_z}"
            );
            villages += usize::from(vanilla.is_some());
        }
    }
    assert!(villages > 0, "no plains village in 36 regions");
}
