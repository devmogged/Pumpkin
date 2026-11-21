use pumpkin_util::math::int_provider::IntProvider;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct BlockColumn {
    pub layers: Vec<BlockColumnLayer>,
    pub direction: String,
    pub allowed_placement: Value,
    pub prioritize_tip: bool,
}

#[derive(Deserialize)]
pub struct BlockColumnLayer {
    #[serde(deserialize_with = "nonnegative_height")]
    pub height: IntProvider,
    pub provider: Value,
}

fn nonnegative_height<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<IntProvider, D::Error> {
    let provider = IntProvider::deserialize(deserializer)?;
    let min = provider.get_min();
    if min < 0 || provider.get_max() < min {
        return Err(serde::de::Error::custom("layer height must be nonnegative"));
    }
    Ok(provider)
}

pub fn probability(config: &Value, key: &str) -> Result<f32, String> {
    let value = config[key]
        .as_f64()
        .ok_or_else(|| format!("missing or invalid {key}"))? as f32;
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{key} must be within 0..=1"))
    }
}

/// A configured feature's dispatch key and parameters.
pub struct ConfiguredFeature<'a> {
    pub kind: &'a str,
    pub config: &'a Value,
}

impl<'a> ConfiguredFeature<'a> {
    pub fn parse(json: &'a Value) -> Result<Self, &'static str> {
        Ok(Self {
            kind: codec_kind(json)?,
            config: json
                .get("config")
                .filter(|value| value.is_object())
                .unwrap_or(json),
        })
    }
}

/// A placed feature's configured feature holder and ordered modifiers.
pub struct PlacedFeature<'a> {
    pub feature: &'a Value,
    pub placement: &'a [Value],
}

impl<'a> PlacedFeature<'a> {
    pub fn parse(json: &'a Value) -> Result<Self, &'static str> {
        let feature = json.get("feature").ok_or("placed feature has no feature")?;
        if !feature.is_string() {
            ConfiguredFeature::parse(feature)?;
        }
        Ok(Self {
            feature,
            placement: json
                .get("placement")
                .and_then(Value::as_array)
                .ok_or("placed feature has no placement list")?,
        })
    }
}

/// Looks up a codec dispatch key, using Minecraft's default namespace.
pub fn codec_kind(json: &Value) -> Result<&str, &'static str> {
    let kind = json
        .get("type")
        .and_then(Value::as_str)
        .ok_or("missing codec type")?;
    Ok(kind.strip_prefix("minecraft:").unwrap_or(kind))
}
