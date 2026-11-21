use pumpkin_data::biome::{Biome, SpawnCosts, SpawnCostsLookup, SpawnGroups, Spawner};
use pumpkin_util::biome::{TemperatureModifier, Weather};
use pumpkin_worldgen_model::biome_data::{
    BiomeFile, TemperatureModifier as FileTemperatureModifier,
};

pub fn build_biome(id: u8, name: &str, json: serde_json::Value) -> Result<Biome, String> {
    let mut biome: BiomeFile = serde_json::from_value(json).map_err(|error| error.to_string())?;
    biome.apply_natural_mob_spawns();
    let temperature_modifier = match biome.temperature_modifier {
        Some(FileTemperatureModifier::Frozen) => TemperatureModifier::Frozen,
        Some(FileTemperatureModifier::None) | None => TemperatureModifier::None,
    };
    let increased_fire_burnout = biome
        .attr_bool("minecraft:gameplay/increased_fire_burnout")
        .unwrap_or(false);
    let snow_golem_melts = biome
        .attr_bool("minecraft:gameplay/snow_golem_melts")
        .unwrap_or(false);
    let can_pillager_patrol_spawn = biome
        .attr_bool("minecraft:gameplay/can_pillager_patrol_spawn")
        .unwrap_or(true);
    let spawners = biome.spawners;
    let group = |entries: Vec<pumpkin_worldgen_model::biome_data::Spawner>| {
        &*Box::leak(
            entries
                .into_iter()
                .map(|entry| Spawner {
                    r#type: Box::leak(entry.r#type.into_boxed_str()),
                    min_count: entry.min_count,
                    max_count: entry.max_count,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    };
    let base = Biome::vanilla_from_id(id);
    Ok(Biome {
        id,
        registry_id: Box::leak(name.to_owned().into_boxed_str()),
        be_network_id: base.unwrap_or(&Biome::PLAINS).be_network_id,
        weather: Weather::new(
            biome.has_precipitation,
            biome.temperature,
            temperature_modifier,
            biome.downfall,
        ),
        // shortcut: runtime carvers and features are wired in E2d.
        features: base.map_or(&[], |base| base.features),
        carvers: base.map_or(&[], |base| base.carvers),
        creature_spawn_probability: biome.creature_spawn_probability.unwrap_or(0.1),
        increased_fire_burnout,
        snow_golem_melts,
        can_pillager_patrol_spawn,
        spawners: SpawnGroups {
            monster: group(spawners.monster),
            ambient: group(spawners.ambient),
            axolotls: group(spawners.axolotls),
            creature: group(spawners.creature),
            misc: group(spawners.misc),
            underground_water_creature: group(spawners.underground_water_creature),
            water_ambient: group(spawners.water_ambient),
            water_creature: group(spawners.water_creature),
        },
        spawn_costs: SpawnCostsLookup::Runtime(
            biome
                .spawn_costs
                .into_iter()
                .map(|(entity, cost)| {
                    (
                        entity
                            .strip_prefix("minecraft:")
                            .unwrap_or(&entity)
                            .to_owned(),
                        SpawnCosts {
                            energy_budget: cost.energy_budget,
                            charge: cost.charge,
                        },
                    )
                })
                .collect(),
        ),
    })
}
