//! Read-only regression evidence for a barrel saved just before server shutdown.
//! Run with PUMP_BARREL_REGION pointing to the existing `.pump` region r.2.2.pump.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use pumpkin_data::{Block, block_properties::BarrelLikeProperties};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::{vector2::Vector2, vector3::Vector3};
use pumpkin_world::chunk::ChunkData;
use ruzstd::decoding::StreamingDecoder;

#[test]
fn inspect_saved_barrel_state() {
    let Ok(file_path) = std::env::var("PUMP_BARREL_REGION") else {
        eprintln!("PUMP_BARREL_REGION not set; inspection not requested");
        return;
    };
    let path = Path::new(&file_path);
    let mut file = File::open(path).expect("open existing region for inspection");
    let mut header = vec![0; 33 * 256];
    file.read_exact(&mut header).unwrap();
    assert_eq!(&header[..8], b"PUMP\0\0\0\x01");

    let x = 1025i32 >> 4;
    let z = 1025i32 >> 4;
    let index = ((z & 31) * 32 + (x & 31)) as usize;
    let table = 256 + index * 8;
    let offset = u32::from_be_bytes(header[table..table + 4].try_into().unwrap());
    let len = u32::from_be_bytes(header[table + 4..table + 8].try_into().unwrap());
    assert!(offset > 0 && len > 12, "target chunk is absent");
    file.seek(SeekFrom::Start(offset as u64 * 256)).unwrap();
    let mut frame = vec![0; len as usize];
    file.read_exact(&mut frame).unwrap();
    let uncompressed_len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    let mut decoder = StreamingDecoder::new(&frame[12..]).unwrap();
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len(), uncompressed_len);
    let chunk = ChunkData::internal_from_pump_bytes(&bytes, Vector2::new(x, z)).unwrap();
    let id = chunk
        .section
        .get_block_absolute_y(1025 & 15, 100, 1025 & 15)
        .unwrap();
    let barrel = Block::from_state_id(id);
    assert_eq!(
        barrel,
        &Block::BARREL,
        "target must remain a vanilla barrel"
    );
    let properties = BarrelLikeProperties::from_state_id(id);
    eprintln!(
        "PERSISTED_BARREL_STATE_ID={} open={} facing={:?}",
        id.as_u16(),
        properties.open,
        properties.facing
    );
    assert!(
        !properties.open,
        "barrel was persisted open despite immediate closure"
    );
    assert_ne!(id.as_u16(), 0, "barrel position unexpectedly became air");
}

#[test]
fn inspect_authentic_desert_pyramid_saved_chunk() {
    let Ok(file_path) = std::env::var("PUMP_DESERT_REGION") else {
        eprintln!("PUMP_DESERT_REGION not set; no inspection requested");
        return;
    };
    let (x, y, z) = (-1366i32, 64i32, -1748i32);
    let (cx, cz) = (x >> 4, z >> 4);
    let mut file = File::open(&file_path).expect("open original region read-only");
    let mut header = vec![0; 33 * 256];
    file.read_exact(&mut header).unwrap();
    assert_eq!(&header[..8], b"PUMP\0\0\0\x01");
    let index = (((cz & 31) * 32) + (cx & 31)) as usize;
    let table = 256 + index * 8;
    let offset = u32::from_be_bytes(header[table..table + 4].try_into().unwrap());
    let len = u32::from_be_bytes(header[table + 4..table + 8].try_into().unwrap());
    assert!(
        offset > 0 && len > 12,
        "desert chunk missing from saved region"
    );
    file.seek(SeekFrom::Start(u64::from(offset) * 256)).unwrap();
    let mut frame = vec![0; len as usize];
    file.read_exact(&mut frame).unwrap();
    let expected_len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    let mut decoder = StreamingDecoder::new(&frame[12..]).unwrap();
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len(), expected_len);
    let chunk = ChunkData::internal_from_pump_bytes(&bytes, Vector2::new(cx, cz)).unwrap();
    let state = chunk
        .section
        .get_block_absolute_y((x & 15) as usize, y, (z & 15) as usize)
        .unwrap();
    let nbt = chunk.pending_block_entities.lock().unwrap();
    let pos = BlockPos(Vector3::new(x, y, z));
    eprintln!(
        "DESERT_SAVED block={} id={} pending={} table={:?} seed={:?}",
        Block::from_state_id(state).name,
        state.as_u16(),
        nbt.len(),
        nbt.get(&pos).and_then(|nbt| nbt.get_string("LootTable")),
        nbt.get(&pos).and_then(|nbt| nbt.get_long("LootTableSeed"))
    );
    for (pos, data) in nbt.iter() {
        eprintln!(
            "DESERT_ENTITY {:?}: {:?} {:?}",
            pos,
            data.get_string("id"),
            data.get_string("LootTable")
        );
    }
}
