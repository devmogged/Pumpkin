use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Value;

use crate::noise_parameter::NoiseParameterJson;

#[derive(Deserialize)]
pub struct NoiseSource {
    pub seed: i64,
    pub noise: NoiseParameterJson,
    #[serde(deserialize_with = "positive_scale")]
    pub scale: f32,
}

#[derive(Deserialize)]
pub struct NoisePalette {
    #[serde(flatten)]
    pub source: NoiseSource,
    #[serde(deserialize_with = "nonempty_states")]
    pub states: Vec<Value>,
}

#[derive(Deserialize)]
pub struct DualNoisePalette {
    #[serde(flatten)]
    pub palette: NoisePalette,
    pub slow_noise: NoiseParameterJson,
    #[serde(deserialize_with = "positive_scale")]
    pub slow_scale: f32,
    #[serde(deserialize_with = "variety_range")]
    pub variety: [u8; 2],
}

#[derive(Deserialize)]
pub struct NoiseThresholdPalette {
    #[serde(flatten)]
    pub source: NoiseSource,
    #[serde(deserialize_with = "threshold")]
    pub threshold: f32,
    #[serde(deserialize_with = "chance")]
    pub high_chance: f32,
    pub default_state: Value,
    #[serde(deserialize_with = "nonempty_states")]
    pub low_states: Vec<Value>,
    #[serde(deserialize_with = "nonempty_states")]
    pub high_states: Vec<Value>,
}

fn positive_scale<'de, D: Deserializer<'de>>(input: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(input)?;
    if value > 0.0 && value.is_finite() {
        Ok(value)
    } else {
        Err(D::Error::custom("noise scale must be positive and finite"))
    }
}

fn nonempty_states<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<Value>, D::Error> {
    let states: Vec<Value> = Vec::deserialize(input)?;
    if states.is_empty() {
        Err(D::Error::custom("noise state palette must not be empty"))
    } else {
        Ok(states)
    }
}

fn threshold<'de, D: Deserializer<'de>>(input: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(input)?;
    if (-1.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(D::Error::custom("noise threshold must be within -1..=1"))
    }
}

fn chance<'de, D: Deserializer<'de>>(input: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(input)?;
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(D::Error::custom("noise chance must be within 0..=1"))
    }
}

fn variety_range<'de, D: Deserializer<'de>>(input: D) -> Result<[u8; 2], D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Interval {
        Single(u8),
        Pair([u8; 2]),
        Named {
            min_inclusive: u8,
            max_inclusive: u8,
        },
    }

    let [min, max] = match Interval::deserialize(input)? {
        Interval::Single(value) => [value, value],
        Interval::Pair(pair) => pair,
        Interval::Named {
            min_inclusive,
            max_inclusive,
        } => [min_inclusive, max_inclusive],
    };
    if min >= 1 && min <= max && max <= 64 {
        Ok([min, max])
    } else {
        Err(D::Error::custom(
            "noise variety must be ordered and within 1..=64",
        ))
    }
}
