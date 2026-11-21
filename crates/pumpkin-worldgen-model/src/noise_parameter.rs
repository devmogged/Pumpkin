use pumpkin_util::noise::perlin::{NoiseNormalization, NormalNoiseSettings};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct NoiseParameterJson {
    #[serde(rename = "firstOctave")]
    first_octave: Option<i32>,
    amplitudes: Option<Vec<f64>>,
    amplitude_modifiers: Option<Vec<f64>>,
    base_amplitude: Option<f64>,
    normalize: Option<Normalization>,
    base_octave: Option<i32>,
    octave_count: Option<usize>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Normalization {
    Flag(bool),
    Named(NamedNormalization),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum NamedNormalization {
    Legacy,
}

impl NoiseParameterJson {
    /// Decodes the sampler settings from the Java 26.3 noise registry.
    pub fn into_runtime_parameters(
        self,
    ) -> Result<(i32, Vec<f64>, NormalNoiseSettings), &'static str> {
        let base_amplitude = self.base_amplitude.unwrap_or(1.0);
        if !(f64::from(1.0e-5f32)..=1_000_000.0).contains(&base_amplitude) {
            return Err("base amplitude exceeds the supported range");
        }
        let normalization = match self.normalize {
            None | Some(Normalization::Flag(true)) => NoiseNormalization::Enabled,
            Some(Normalization::Flag(false)) => NoiseNormalization::Disabled,
            Some(Normalization::Named(NamedNormalization::Legacy)) => NoiseNormalization::Legacy,
        };
        let (octave, amplitudes) = self.into_parameters()?;
        Ok((
            octave,
            amplitudes,
            NormalNoiseSettings {
                base_amplitude,
                normalization,
            },
        ))
    }

    /// Resolves both vanilla noise parameter formats.
    pub fn into_parameters(self) -> Result<(i32, Vec<f64>), &'static str> {
        let first = self.first_octave.or(self.base_octave).unwrap_or(-7);
        let amplitudes = self
            .amplitudes
            .or(self.amplitude_modifiers)
            .filter(|a| !a.is_empty());
        let len = amplitudes
            .as_ref()
            .map_or(self.octave_count.unwrap_or(1), Vec::len);
        if !(1..=32).contains(&len)
            || !(-32..=32).contains(&first)
            || self.octave_count.is_some_and(|count| count != len)
            || amplitudes.as_ref().is_some_and(|values| {
                values
                    .iter()
                    .any(|&value| !(0.0..=1_000_000.0).contains(&value))
            })
        {
            return Err("noise parameters exceed the supported octave range");
        }
        Ok((first, amplitudes.unwrap_or_else(|| vec![1.0; len])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_util::{
        noise::perlin::{Noise, NormalNoise},
        random::xoroshiro128::Xoroshiro,
    };

    #[test]
    fn runtime_matches_java_normalization_modes() -> Result<(), Box<dyn std::error::Error>> {
        let positions = [
            (0.0, 0.0, 0.0),
            (17.25, -18.75, -32.0),
            (1024.0, 64.0, 1024.0),
        ];
        // NormalNoise.DIRECT_CODEC and XoroshiroRandomSource(42) in Java 26.3.
        for (mode, expected, max) in [
            (None, [0xbe2a_7b1f, 0xbd37_55e4, 0xbe1f_f67e], 0x4002_2222),
            (
                Some(serde_json::json!(false)),
                [0xbe9f_d36e, 0xbdab_e082, 0xbe95_f716],
                0x4074_0000,
            ),
            (
                Some(serde_json::json!("legacy")),
                [0xbc64_48ba, 0xbb75_7f41, 0xbc56_3326],
                0x3e2e_4199,
            ),
        ] {
            let mut json = serde_json::json!({
                "base_octave": -8,
                "octave_count": 4,
                "amplitude_modifiers": [1, 1, 0.5, 2.25],
            });
            if let Some(mode) = mode {
                if mode == "legacy" {
                    json["base_amplitude"] = serde_json::json!(0.3);
                }
                json["normalize"] = mode;
            }
            let noise: NoiseParameterJson = serde_json::from_value(json)?;
            let (octave, amplitudes, settings) = noise.into_runtime_parameters()?;
            let noise = NormalNoise::from_parameters(
                &mut Xoroshiro::from_seed(42),
                octave,
                &amplitudes,
                settings,
                false,
            );
            assert_eq!(
                positions.map(|(x, y, z)| noise.get(x, y, z).to_bits()),
                expected
            );
            assert_eq!(noise.max_value().to_bits(), max);
        }
        Ok(())
    }
}
