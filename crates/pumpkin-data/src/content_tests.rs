use super::{Block, BlockDefinition, Item, ItemDefinition, Registry, StateDefinition};

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
    assert!(second.state(old_shaft.as_u16()).is_none());
    assert_eq!(
        second.state(old_cog.as_u16()).map(|s| s.state.id),
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
        third.state(old_shaft.as_u16()).map(|s| s.state.id),
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
        states: vec![StateDefinition {
            properties: std::collections::BTreeMap::new(),
            physical: Block::STONE.default_state,
            carrier: Block::NOTE_BLOCK.default_state.id,
            carrier_replacement: None,
            shape: Vec::new(),
        }],
    }
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
    };
    assert!(Registry::open(&file, &[definition("test:shaft")], &[clash]).is_err());
    Ok(())
}
