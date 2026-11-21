use serde::Deserialize;

use crate::noise_settings::ParameterStruct;

/// A direct entry in a Minecraft multi-noise biome source.
#[derive(Deserialize)]
pub struct ClimateEntry {
    pub biome: String,
    parameters: ClimatePoint,
}

#[derive(Deserialize)]
struct ClimatePoint {
    temperature: ParameterStruct,
    humidity: ParameterStruct,
    continentalness: ParameterStruct,
    erosion: ParameterStruct,
    depth: ParameterStruct,
    weirdness: ParameterStruct,
    offset: f32,
}

impl ClimateEntry {
    /// Returns the quantized climate ranges after validating the vanilla codec's bounds.
    pub fn parameter_space(&self) -> Result<[(i64, i64); 7], &'static str> {
        let point = &self.parameters;
        let parameters = [
            &point.temperature,
            &point.humidity,
            &point.continentalness,
            &point.erosion,
            &point.depth,
            &point.weirdness,
        ];
        let mut ranges = [(0, 0); 7];
        for (index, parameter) in parameters.into_iter().enumerate() {
            let (min, max) = parameter.bounds();
            if !(-2.0..=2.0).contains(&min) || !(min..=2.0).contains(&max) {
                return Err("climate range bounds must be ordered and within -2..=2");
            }
            ranges[index] = parameter.quantized();
        }
        if !(0.0..=1.0).contains(&point.offset) {
            return Err("climate offset must be within 0..=1");
        }
        ranges[6] = ParameterStruct::Point(point.offset).quantized();
        Ok(ranges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_quantizes_vanilla_climate_intervals() -> Result<(), serde_json::Error> {
        let mut input = serde_json::json!({
            "biome": "test:example",
            "parameters": {
                "temperature": -0.50019,
                "humidity": [-1.0, 1.0],
                "continentalness": { "min": -1.2, "max": -1.05 },
                "erosion": 0.0,
                "depth": [0.0, 0.1],
                "weirdness": [-1.0, -0.4],
                "offset": 0.375
            }
        });
        let entry: ClimateEntry = serde_json::from_value(input.clone())?;
        assert_eq!(
            entry.parameter_space(),
            Ok([
                (-5001, -5001),
                (-10000, 10000),
                (-12000, -10500),
                (0, 0),
                (0, 1000),
                (-10000, -4000),
                (3750, 3750)
            ])
        );

        input["parameters"]["humidity"] = serde_json::json!([0.5, -0.5]);
        let entry: ClimateEntry = serde_json::from_value(input.clone())?;
        assert!(entry.parameter_space().is_err());
        input["parameters"]["humidity"] = serde_json::json!(0.0);
        input["parameters"]["offset"] = serde_json::json!(1.000001);
        let entry: ClimateEntry = serde_json::from_value(input)?;
        assert!(entry.parameter_space().is_err());
        Ok(())
    }
}
