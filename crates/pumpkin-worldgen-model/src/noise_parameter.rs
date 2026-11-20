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
#[serde(rename_all = "snake_case")]
enum Normalization {
    Enabled,
    Disabled,
    Legacy,
}

impl NoiseParameterJson {
    /// Checks whether the current sampler can represent this pack's noise.
    pub fn into_runtime_parameters(self) -> Result<(i32, Vec<f64>), &'static str> {
        if !matches!(self.normalize, Some(Normalization::Legacy))
            || self
                .base_amplitude
                .is_some_and(|amplitude| amplitude != 1.0)
        {
            return Err(
                "only legacy normalization with base_amplitude 1 is supported for overridden noises",
            );
        }
        self.into_parameters()
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
