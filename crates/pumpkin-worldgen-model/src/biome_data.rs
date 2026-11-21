use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;
#[cfg(feature = "codegen")]
use {proc_macro2::TokenStream, quote::quote};

fn deserialize_carvers<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match Option::<StringOrVec>::deserialize(deserializer)? {
        Some(StringOrVec::String(s)) => Ok(vec![s]),
        Some(StringOrVec::Vec(v)) => Ok(v),
        None => Ok(Vec::new()),
    }
}

/// Raw deserialization shape for a single biome entry from `biome.json`.
#[derive(Deserialize)]
pub struct BiomeFile {
    /// Whether this biome has precipitation (rain or snow).
    pub has_precipitation: bool,
    /// Base temperature of the biome affecting weather and foliage color.
    pub temperature: f32,
    /// Downfall value affecting how wet the biome is (influences snow/rain).
    pub downfall: f32,
    /// Optional modifier that changes how temperature is applied.
    pub temperature_modifier: Option<TemperatureModifier>,
    #[serde(default, deserialize_with = "deserialize_carvers")]
    pub carvers: Vec<String>,
    /// Nested lists of feature resource-location strings applied during world generation.
    pub features: Vec<Vec<String>>,
    /// Probability per chunk tick that a creature spawns, if not overridden per-biome.
    pub creature_spawn_probability: Option<f32>,
    /// Spawn group data for each entity category in this biome.
    #[serde(default)]
    pub spawners: SpawnGroups,
    /// Per-entity spawn cost budget entries, keyed by namespaced entity ID.
    #[serde(default)]
    pub spawn_costs: BTreeMap<String, SpawnCosts>,
    /// Environment attributes; 26.3 stores natural mob spawns here.
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    /// Numeric registry ID assigned to this biome.
    #[serde(default)]
    pub id: u8,
}

impl BiomeFile {
    #[must_use]
    pub fn attr_bool(&self, key: &str) -> Option<bool> {
        let val = self.attributes.get(key)?;
        val.as_bool()
            .or_else(|| val.get("argument").and_then(Value::as_bool))
    }

    fn attr_f32(&self, key: &str) -> Option<f32> {
        let val = self.attributes.get(key)?;
        val.as_f64()
            .or_else(|| val.get("argument").and_then(Value::as_f64))
            .map(|v| v as f32)
    }

    pub fn apply_natural_mob_spawns(&mut self) {
        if self.creature_spawn_probability.is_none() {
            self.creature_spawn_probability =
                self.attr_f32("minecraft:gameplay/creature_world_gen_spawn_probability");
        }
        let Some(attr) = self.attributes.get("minecraft:gameplay/natural_mob_spawns") else {
            return;
        };
        let Some(argument) = attr.get("argument") else {
            return;
        };
        if let Some(spawns) = argument.get("spawns_by_category")
            && let Ok(groups) = serde_json::from_value::<SpawnGroups>(spawns.clone())
        {
            self.spawners = groups;
        }
        if let Some(costs) = argument.get("spawn_costs")
            && let Ok(spawn_costs) = serde_json::from_value(costs.clone())
        {
            self.spawn_costs = spawn_costs;
        }
    }
}

/// Spawn group data for all entity categories within a biome.
#[derive(Deserialize, Default, PartialEq, Eq, Hash)]
pub struct SpawnGroups {
    /// Hostile mob spawners for this biome.
    #[serde(default)]
    pub monster: Vec<Spawner>,
    /// Ambient creature spawners (e.g. bats) for this biome.
    #[serde(default)]
    pub ambient: Vec<Spawner>,
    /// Axolotl spawners for this biome.
    #[serde(default)]
    pub axolotls: Vec<Spawner>,
    /// Passive creature spawners (e.g. cows, sheep) for this biome.
    #[serde(default)]
    pub creature: Vec<Spawner>,
    /// Miscellaneous entity spawners for this biome.
    #[serde(default)]
    pub misc: Vec<Spawner>,
    /// Underground water creature spawners (e.g. glow squid) for this biome.
    #[serde(default)]
    pub underground_water_creature: Vec<Spawner>,
    /// Water ambient spawners (e.g. fish) for this biome.
    #[serde(default)]
    pub water_ambient: Vec<Spawner>,
    /// Water creature spawners (e.g. dolphins) for this biome.
    #[serde(default)]
    pub water_creature: Vec<Spawner>,
}

/// A single entity spawner entry within a spawn group, as defined in `biome.json`.
#[derive(Hash, PartialEq, Eq)]
pub struct Spawner {
    /// Namespaced entity type ID (e.g. `"minecraft:zombie"`).
    pub r#type: String,
    /// Minimum number of entities in a spawn group.
    pub min_count: i32,
    /// Maximum number of entities in a spawn group.
    pub max_count: i32,
}

impl<'de> Deserialize<'de> for Spawner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            r#type: String,
            #[serde(default, alias = "minCount")]
            min_count: Option<i32>,
            #[serde(default, alias = "maxCount")]
            max_count: Option<i32>,
            #[serde(default)]
            count: Option<Value>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let (min_count, max_count) = match raw.count {
            Some(Value::Number(n)) => {
                let value = n.as_i64().unwrap_or(1) as i32;
                (value, value)
            }
            Some(Value::Object(object)) => {
                let min = object
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(1) as i32;
                let max = object
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(min as i64) as i32;
                (min, max)
            }
            _ => (raw.min_count.unwrap_or(1), raw.max_count.unwrap_or(1)),
        };
        Ok(Self {
            r#type: raw.r#type,
            min_count,
            max_count,
        })
    }
}

#[cfg(feature = "codegen")]
impl Spawner {
    /// Converts this spawner entry into a `TokenStream` for use in generated code.
    #[must_use]
    pub fn to_tokens(&self) -> TokenStream {
        let r#type = &self.r#type;
        let min_count = &self.min_count;
        let max_count = &self.max_count;
        quote! {
            Spawner {
                r#type: #r#type,
                min_count: #min_count,
                max_count: #max_count,
            }
        }
    }
}

/// Mob spawn cost data controlling how this entity affects biome spawn budgets.
#[derive(Deserialize, PartialEq)]
pub struct SpawnCosts {
    /// Maximum energy this entity type may consume from the spawn budget.
    pub energy_budget: f64,
    /// Energy cost charged to the budget each time this entity spawns.
    pub charge: f64,
}

#[cfg(feature = "codegen")]
impl SpawnCosts {
    /// Converts these spawn costs into a `TokenStream` for use in generated code.
    #[must_use]
    pub fn to_tokens(&self) -> TokenStream {
        let energy_budget = &self.energy_budget;
        let charge = &self.charge;
        quote! {
            SpawnCosts {
                energy_budget: #energy_budget,
                charge: #charge,
            }
        }
    }
}

/// Optional modifier that adjusts how biome temperature behaves (e.g. frozen biomes).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureModifier {
    /// No modification; temperature is used as-is.
    None,
    /// Temperature is forced below freezing regardless of the base value.
    Frozen,
}
