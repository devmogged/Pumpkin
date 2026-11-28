use super::{
    Block, BlockDefinition, BlockMaterial, Item, ItemDefinition, Registry, StateDefinition,
};

#[test]
fn custom_physics_overrides_template_without_mutating_vanilla()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("registry.dat");
    let properties = super::BlockPhysics {
        hardness: 50.0,
        blast_resistance: 50.0,
        tool_required: true,
        sound_type: Block::NETHERITE_BLOCK.default_state.sound_type,
    };
    let registry = Registry::open_with_properties(
        &file,
        &[definition("test:alloy")],
        &[],
        |_, _| None,
        |_| Some(properties),
    )?;
    let block = registry.block("test:alloy").ok_or("missing alloy")?;
    assert_eq!(block.hardness, 50.0);
    assert_eq!(block.blast_resistance, 50.0);
    assert_eq!(block.default_state.hardness, 50.0);
    assert!(block.default_state.tool_required());
    assert_eq!(block.default_state.sound_type, properties.sound_type);
    assert_eq!(Block::NETHERITE_BLOCK.blast_resistance, 1200.0);
    let invalid = super::BlockPhysics {
        hardness: f32::NAN,
        ..properties
    };
    assert!(
        Registry::open_with_properties(
            &file,
            &[definition("test:alloy")],
            &[],
            |_, _| None,
            |_| Some(invalid),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn removed_slots_remain_reserved_after_restart() -> Result<(), Box<dyn std::error::Error>> {
    let world = tempfile::tempdir()?;
    let file = world.path().join("moddedmc/registry.dat");
    let first = Registry::open(
        &file,
        &[definition("test:shaft"), definition("test:cog")],
        &[],
    )?;
    let old_shaft = first
        .block("test:shaft")
        .ok_or("missing shaft")?
        .default_state
        .id;
    let old_cog = first
        .block("test:cog")
        .ok_or("missing cog")?
        .default_state
        .id;
    let second = Registry::open(
        &file,
        &[definition("test:motor"), definition("test:cog")],
        &[],
    )?;
    assert!(second.state(old_shaft.as_u32()).is_none());
    assert_eq!(
        second.state(old_cog.as_u32()).map(|s| s.state.id),
        Some(old_cog)
    );
    assert!(
        second
            .block("test:motor")
            .ok_or("missing motor")?
            .default_state
            .id
            > old_cog
    );
    let third = Registry::open(&file, &[definition("test:shaft")], &[])?;
    assert_eq!(
        third.state(old_shaft.as_u32()).map(|s| s.state.id),
        Some(old_shaft)
    );
    Ok(())
}

fn definition(name: &str) -> BlockDefinition {
    BlockDefinition {
        name: name.into(),
        display_name: name.into(),
        entity: None,
        item_carrier: &Item::PAPER,
        material: None,
        states: vec![StateDefinition {
            random_ticks: false,
            properties: std::collections::BTreeMap::new(),
            physical: Block::STONE.default_state,
            carrier: Block::NOTE_BLOCK.default_state.id,
            carrier_replacement: None,
            shape: Vec::new(),
        }],
    }
}

#[test]
fn state_allocator_crosses_u16_and_preserves_saved_ids() -> Result<(), Box<dyn std::error::Error>> {
    let world = tempfile::tempdir()?;
    let file = world.path().join("registry.dat");
    std::fs::write(
        &file,
        r#"{"blocks":{},"states":{"test:old{}":65535},"items":{}}"#,
    )?;
    let definitions = [definition("test:old"), definition("test:wide")];
    let first = Registry::open(&file, &definitions, &[])?;
    let old = first
        .block("test:old")
        .ok_or("old state missing")?
        .default_state
        .id;
    let wide = first
        .block("test:wide")
        .ok_or("wide state missing")?
        .default_state
        .id;
    assert_eq!(old.as_u32(), 65535);
    assert_eq!(wide.as_u32(), 65536);
    assert_eq!(
        first
            .state(wide.as_u32())
            .ok_or("wide lookup missing")?
            .state
            .id,
        wide
    );
    let second = Registry::open(&file, &[definition("test:wide")], &[])?;
    assert!(second.state(old.as_u32()).is_none());
    assert_eq!(
        second
            .block("test:wide")
            .ok_or("wide state lost")?
            .default_state
            .id,
        wide
    );
    let third = Registry::open(&file, &definitions, &[])?;
    assert_eq!(
        third
            .block("test:old")
            .ok_or("old state lost")?
            .default_state
            .id,
        old
    );
    assert_eq!(
        third
            .block("test:wide")
            .ok_or("wide state lost")?
            .default_state
            .id,
        wide
    );
    Ok(())
}

#[test]
fn a_slab_can_copy_a_different_blocks_material() -> Result<(), Box<dyn std::error::Error>> {
    use crate::map_color::MapColor;

    let world = tempfile::tempdir()?;
    let mut slab = definition("test:calcite_slab");
    slab.states[0].physical = Block::STONE_SLAB.default_state;
    slab.material = Some(BlockMaterial {
        template: &Block::CALCITE,
        hardness: Some(1.25),
        map_color: Some(MapColor::TERRACOTTA_YELLOW.id),
    });
    let registry = Registry::open(&world.path().join("registry.dat"), &[slab], &[])?;
    let block = registry.block("test:calcite_slab").ok_or("missing slab")?;
    let state = block.default_state;
    assert_eq!(
        state.collision_shapes,
        Block::STONE_SLAB.default_state.collision_shapes
    );
    assert_eq!(state.get_sound_type().name, "CALCITE");
    assert_eq!(state.hardness, 1.25);
    assert_eq!(block.blast_resistance, 0.75);
    assert_eq!(block.map_color, MapColor::TERRACOTTA_YELLOW.id);
    Ok(())
}

#[test]
fn explicit_physics_preserve_material_and_collision() -> Result<(), Box<dyn std::error::Error>> {
    use crate::map_color::MapColor;

    let world = tempfile::tempdir()?;
    let mut slab = definition("test:alloy_slab");
    slab.states[0].physical = Block::STONE_SLAB.default_state;
    slab.material = Some(BlockMaterial {
        template: &Block::CALCITE,
        hardness: Some(1.25),
        map_color: Some(MapColor::TERRACOTTA_YELLOW.id),
    });
    let properties = super::BlockPhysics {
        hardness: 50.0,
        blast_resistance: 50.0,
        tool_required: true,
        sound_type: Block::NETHERITE_BLOCK.default_state.sound_type,
    };
    let registry = Registry::open_with_properties(
        &world.path().join("registry.dat"),
        &[slab],
        &[],
        |_, _| None,
        |_| Some(properties),
    )?;
    let block = registry.block("test:alloy_slab").ok_or("missing slab")?;
    assert_eq!(
        block.default_state.collision_shapes,
        Block::STONE_SLAB.default_state.collision_shapes
    );
    assert_eq!(block.default_state.sound_type, properties.sound_type);
    assert_eq!(block.default_state.hardness, 50.0);
    assert!(block.default_state.tool_required());
    assert_eq!(block.hardness, 50.0);
    assert_eq!(block.blast_resistance, 50.0);
    assert_eq!(block.map_color, MapColor::TERRACOTTA_YELLOW.id);
    Ok(())
}

#[test]
fn recipe_ids_match_mod_and_vanilla_items() -> Result<(), Box<dyn std::error::Error>> {
    let world = tempfile::tempdir()?;
    let registry = Registry::open(
        &world.path().join("moddedmc/registry.dat"),
        &[definition("test:stone")],
        &[],
    )?;
    let block = registry.block("test:stone").ok_or("missing block")?;
    let item = registry
        .items
        .get(&block.item_id)
        .ok_or("missing item")?
        .item;
    assert!(item.has_id("test:stone"));
    assert!(!item.has_id("minecraft:stone"));
    assert!(!Item::STONE.has_id("test:stone"));
    assert!(Item::STONE.has_id("minecraft:stone"));
    assert_eq!(block.loot_table, Some("test:blocks/stone"));
    Ok(())
}

#[test]
fn standalone_items_keep_their_ids_and_carrier_components() -> Result<(), Box<dyn std::error::Error>>
{
    let world = tempfile::tempdir()?;
    let file = world.path().join("moddedmc/registry.dat");
    let alloy = || ItemDefinition {
        name: "test:alloy".into(),
        display_name: "Alloy".into(),
        carrier: &Item::IRON_INGOT,
        components: Vec::new(),
        client_components: Vec::new(),
    };
    let first = Registry::open(&file, &[definition("test:shaft")], &[alloy()])?;
    let id = first
        .item_names
        .get("test:alloy")
        .ok_or("missing alloy")?
        .id;
    let nugget = ItemDefinition {
        name: "test:nugget".into(),
        display_name: "Nugget".into(),
        carrier: &Item::IRON_NUGGET,
        components: Vec::new(),
        client_components: Vec::new(),
    };
    let second = Registry::open(&file, &[definition("test:shaft")], &[nugget, alloy()])?;
    let item = *second.item_names.get("test:alloy").ok_or("missing alloy")?;
    assert_eq!(item.id, id);
    let entry = second.items.get(&id).ok_or("missing entry")?;
    assert!(entry.block.is_none());
    let ids = |item: &Item| {
        item.components
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(item), ids(&Item::IRON_INGOT));

    let clash = ItemDefinition {
        name: "test:shaft".into(),
        display_name: "Shaft".into(),
        carrier: &Item::STICK,
        components: Vec::new(),
        client_components: Vec::new(),
    };
    assert!(Registry::open(&file, &[definition("test:shaft")], &[clash]).is_err());
    Ok(())
}
