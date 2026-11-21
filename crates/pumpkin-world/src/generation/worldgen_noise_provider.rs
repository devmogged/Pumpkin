use pumpkin_data::BlockState;
use pumpkin_worldgen_model::state_provider::{
    DualNoisePalette, NoisePalette, NoiseSource, NoiseThresholdPalette,
};
use serde_json::Value;

use super::{
    block_state_provider::{
        BlockStateProvider, DualNoiseBlockStateProvider, NoiseBlockStateProvider,
        NoiseBlockStateProviderBase, NoiseThresholdBlockStateProvider,
    },
    worldgen_placement::{read, state},
};

pub(super) fn compile(kind: &str, value: &Value) -> Result<BlockStateProvider, String> {
    Ok(match kind {
        "noise" | "noise_provider" => BlockStateProvider::NoiseProvider(palette(read(value)?)?),
        "dual_noise" | "dual_noise_provider" => {
            let config: DualNoisePalette = read(value)?;
            let slow_noise = sampler(NoiseSource {
                seed: config.palette.source.seed,
                noise: config.slow_noise,
                scale: config.slow_scale,
            })?;
            BlockStateProvider::DualNoise(DualNoiseBlockStateProvider {
                base: palette(config.palette)?,
                variety: config.variety,
                slow_noise,
            })
        }
        "noise_threshold" | "noise_threshold_provider" => {
            let config: NoiseThresholdPalette = read(value)?;
            BlockStateProvider::NoiseThreshold(NoiseThresholdBlockStateProvider {
                base: sampler(config.source)?,
                threshold: config.threshold,
                high_chance: config.high_chance,
                default_state: state(&config.default_state)?,
                low_states: states(&config.low_states)?,
                high_states: states(&config.high_states)?,
            })
        }
        _ => return Err(format!("unsupported block-state provider {kind}")),
    })
}

fn palette(config: NoisePalette) -> Result<NoiseBlockStateProvider, String> {
    Ok(NoiseBlockStateProvider {
        base: sampler(config.source)?,
        states: states(&config.states)?,
    })
}

fn states(values: &[Value]) -> Result<Vec<&'static BlockState>, String> {
    values.iter().map(state).collect()
}

fn sampler(config: NoiseSource) -> Result<NoiseBlockStateProviderBase, String> {
    let (octave, amplitudes, settings) = config.noise.into_runtime_parameters()?;
    Ok(NoiseBlockStateProviderBase::new(
        config.seed,
        octave,
        &amplitudes,
        settings,
        config.scale,
    ))
}
