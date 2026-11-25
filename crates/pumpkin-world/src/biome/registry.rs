use pumpkin_data::{
    biome::{Biome, SpawnCosts, SpawnCostsLookup, SpawnGroups, Spawner},
    carver::CarverConfig,
    placed_feature::PlacedFeature,
};
use pumpkin_util::biome::{TemperatureModifier, Weather};
use pumpkin_worldgen_model::biome_data::{
    BiomeFile, TemperatureModifier as FileTemperatureModifier,
};

pub fn build_biome(
    id: u8,
    name: &str,
    json: serde_json::Value,
    carver: impl FnMut(&str) -> Option<&'static CarverConfig>,
    mut placed_feature: impl FnMut(&str) -> Option<PlacedFeature>,
) -> Result<Biome, String> {
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
    let carvers = biome.carvers.iter().map(String::as_str).filter_map(carver);
    let group = |entries: Vec<pumpkin_worldgen_model::biome_data::Spawner>| {
        &*Box::leak(
            entries
                .into_iter()
                .map(|entry| Spawner {
                    r#type: Box::leak(entry.r#type.into_boxed_str()),
                    min_count: entry.min_count,
                    max_count: entry.max_count,
                    weight: entry.weight,
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
        features: Box::leak(
            biome
                .features
                .into_iter()
                .map(|step| {
                    &*Box::leak(
                        step.iter()
                            .map(String::as_str)
                            .filter_map(&mut placed_feature)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        carvers: Box::leak(carvers.collect::<Vec<_>>().into_boxed_slice()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datapack_biome_preserves_placed_features_by_step() {
        let biome = build_biome(
            250,
            "fixture:features",
            serde_json::json!({
                "has_precipitation": false,
                "temperature": 0.5,
                "downfall": 0.0,
                "carvers": [],
                "features": [["minecraft:lake_lava_underground"], [], ["minecraft:ore_diamond", "minecraft:ore_gold"]],
            }),
            CarverConfig::from_name,
            PlacedFeature::from_name,
        )
        .unwrap();
        assert_eq!(
            biome.features,
            &[
                &[PlacedFeature::LakeLavaUnderground][..],
                &[],
                &[PlacedFeature::OreDiamond, PlacedFeature::OreGold],
            ],
        );
    }

    #[test]
    fn datapack_biome_applies_its_carvers_in_order() {
        let biome = build_biome(
            250,
            "fixture:ravines",
            serde_json::json!({
                "has_precipitation": false,
                "temperature": 0.5,
                "downfall": 0.0,
                "carvers": ["minecraft:canyon", "minecraft:cave"],
                "features": [],
            }),
            CarverConfig::from_name,
            PlacedFeature::from_name,
        )
        .unwrap();
        assert_eq!(biome.carvers.len(), 2);
        assert!(matches!(
            biome.carvers[0].additional,
            pumpkin_data::carver::CarverAdditionalConfig::Canyon(_)
        ));
        assert!(matches!(
            biome.carvers[1].additional,
            pumpkin_data::carver::CarverAdditionalConfig::Cave(_)
        ));
    }
}
