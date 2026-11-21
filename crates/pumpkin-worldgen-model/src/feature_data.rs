use serde_json::Value;

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
