use std::num::NonZeroU32;

use pumpkin_util::math::{float_provider::FloatProvider, int_provider::IntProvider};
use serde::Deserialize;

use crate::material::YOffsetStruct;

#[derive(Deserialize)]
pub struct CarverJson {
    probability: f32,
    y: HeightJson,
    #[serde(flatten)]
    kind: Kind,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Kind {
    #[serde(rename = "minecraft:cave", alias = "cave")]
    Cave(Cave),
    #[serde(rename = "minecraft:canyon", alias = "canyon")]
    Canyon(Canyon),
}

#[derive(Deserialize)]
struct Cave {
    count: IntProvider,
    horizontal_radius_multiplier: FloatProvider,
    vertical_radius_multiplier: FloatProvider,
    floor_level: FloatProvider,
    room_vertical_radius_multiplier: FloatProvider,
    #[serde(default = "one")]
    start_vertical_radius_multiplier: FloatProvider,
    thickness: FloatProvider,
    #[serde(default)]
    weird_thickness_bias: bool,
}

const fn one() -> FloatProvider {
    FloatProvider::Constant(1.0)
}

#[derive(Deserialize)]
struct Canyon {
    vertical_rotation: FloatProvider,
    shape: CanyonShape,
}

#[derive(Deserialize)]
struct CanyonShape {
    distance_factor: FloatProvider,
    thickness: FloatProvider,
    width_smoothness: i32,
    horizontal_radius_factor: FloatProvider,
    vertical_radius_default_factor: f32,
    vertical_radius_center_factor: f32,
    y_scale: FloatProvider,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum HeightJson {
    Anchor(YOffsetStruct),
    Provider(HeightDistribution),
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum HeightDistribution {
    #[serde(rename = "minecraft:constant", alias = "constant")]
    Constant { value: YOffsetStruct },
    #[serde(rename = "minecraft:uniform", alias = "uniform")]
    Uniform {
        min_inclusive: YOffsetStruct,
        max_inclusive: YOffsetStruct,
    },
    #[serde(rename = "minecraft:trapezoid", alias = "trapezoid")]
    Trapezoid {
        min_inclusive: YOffsetStruct,
        max_inclusive: YOffsetStruct,
        plateau: Option<i32>,
    },
    #[serde(rename = "minecraft:biased_to_bottom", alias = "biased_to_bottom")]
    BiasedToBottom {
        min_inclusive: YOffsetStruct,
        max_inclusive: YOffsetStruct,
        inner: Option<NonZeroU32>,
    },
    #[serde(
        rename = "minecraft:very_biased_to_bottom",
        alias = "very_biased_to_bottom"
    )]
    VeryBiasedToBottom {
        min_inclusive: YOffsetStruct,
        max_inclusive: YOffsetStruct,
        inner: Option<NonZeroU32>,
    },
}

impl CarverJson {
    /// Parses a 26.3 entry and checks the constraints used by its carver.
    pub fn parse(json: serde_json::Value) -> Result<Self, String> {
        let carver: Self = serde_json::from_value(json).map_err(|error| error.to_string())?;
        if !(0.0..=1.0).contains(&carver.probability) {
            return Err("carver probability must be within 0..=1".into());
        }
        match &carver.kind {
            Kind::Cave(cave) if cave.count.get_min() < 0 => {
                Err("cave count cannot be negative".into())
            }
            Kind::Canyon(canyon) if canyon.shape.width_smoothness <= 0 => {
                Err("canyon width_smoothness must be positive".into())
            }
            _ => Ok(carver),
        }
    }
}

#[cfg(feature = "codegen")]
impl CarverJson {
    #[must_use]
    pub fn emit(&self, id: &str) -> proc_macro2::TokenStream {
        use quote::quote;
        let probability = self.probability;
        let y = match &self.y {
            HeightJson::Anchor(anchor)
            | HeightJson::Provider(HeightDistribution::Constant { value: anchor }) => {
                quote!(HeightProvider::Constant(#anchor))
            }
            HeightJson::Provider(HeightDistribution::Uniform {
                min_inclusive,
                max_inclusive,
            }) => {
                quote!(HeightProvider::Uniform(UniformHeightProvider { min_inclusive: #min_inclusive, max_inclusive: #max_inclusive }))
            }
            HeightJson::Provider(HeightDistribution::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            }) => {
                let plateau = plateau.map_or_else(|| quote!(None), |p| quote!(Some(#p)));
                quote!(HeightProvider::Trapezoid(TrapezoidHeightProvider { min_inclusive: #min_inclusive, max_inclusive: #max_inclusive, plateau: #plateau }))
            }
            HeightJson::Provider(HeightDistribution::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            }) => {
                let inner = inner.map_or_else(
                    || quote!(None),
                    |n| {
                        let n = n.get();
                        quote!(std::num::NonZero::new(#n))
                    },
                );
                quote!(HeightProvider::BiasedToBottom(BiasedToBottomHeightProvider { min_inclusive: #min_inclusive, max_inclusive: #max_inclusive, inner: #inner }))
            }
            HeightJson::Provider(HeightDistribution::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            }) => {
                let inner = inner.map_or_else(
                    || quote!(None),
                    |n| {
                        let n = n.get();
                        quote!(std::num::NonZero::new(#n))
                    },
                );
                quote!(HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeightProvider { min_inclusive: #min_inclusive, max_inclusive: #max_inclusive, inner: #inner }))
            }
        };
        let additional = match &self.kind {
            Kind::Cave(Cave {
                count,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                floor_level,
                room_vertical_radius_multiplier,
                start_vertical_radius_multiplier,
                thickness,
                weird_thickness_bias,
            }) => quote!(CarverAdditionalConfig::Cave(CaveCarverConfig {
                count: #count,
                horizontal_radius_multiplier: #horizontal_radius_multiplier,
                vertical_radius_multiplier: #vertical_radius_multiplier,
                floor_level: #floor_level,
                room_vertical_radius_multiplier: #room_vertical_radius_multiplier,
                start_vertical_radius_multiplier: #start_vertical_radius_multiplier,
                thickness: #thickness,
                weird_thickness_bias: #weird_thickness_bias,
            })),
            Kind::Canyon(Canyon {
                vertical_rotation,
                shape:
                    CanyonShape {
                        distance_factor,
                        thickness,
                        width_smoothness,
                        horizontal_radius_factor,
                        vertical_radius_default_factor,
                        vertical_radius_center_factor,
                        y_scale,
                    },
            }) => quote!(CarverAdditionalConfig::Canyon(CanyonCarverConfig {
                vertical_rotation: #vertical_rotation,
                shape: CanyonShapeConfig {
                    distance_factor: #distance_factor,
                    thickness: #thickness,
                    width_smoothness: #width_smoothness,
                    horizontal_radius_factor: #horizontal_radius_factor,
                    vertical_radius_default_factor: #vertical_radius_default_factor,
                    vertical_radius_center_factor: #vertical_radius_center_factor,
                    y_scale: #y_scale,
                },
            })),
        };
        quote!(CarverConfig { registry_id: #id, probability: #probability, y: #y, additional: #additional })
    }
}

#[cfg(feature = "runtime")]
impl CarverJson {
    #[must_use]
    pub fn build(self, registry_id: &'static str) -> pumpkin_data::carver::CarverConfig {
        use pumpkin_data::carver::{
            BiasedToBottomHeightProvider, CanyonCarverConfig, CanyonShapeConfig,
            CarverAdditionalConfig, CarverConfig, CaveCarverConfig, HeightProvider,
            TrapezoidHeightProvider, UniformHeightProvider, VeryBiasedToBottomHeightProvider,
        };
        let y = match self.y {
            HeightJson::Anchor(anchor)
            | HeightJson::Provider(HeightDistribution::Constant { value: anchor }) => {
                HeightProvider::Constant(anchor.into_offset())
            }
            HeightJson::Provider(HeightDistribution::Uniform {
                min_inclusive,
                max_inclusive,
            }) => HeightProvider::Uniform(UniformHeightProvider {
                min_inclusive: min_inclusive.into_offset(),
                max_inclusive: max_inclusive.into_offset(),
            }),
            HeightJson::Provider(HeightDistribution::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            }) => HeightProvider::Trapezoid(TrapezoidHeightProvider {
                min_inclusive: min_inclusive.into_offset(),
                max_inclusive: max_inclusive.into_offset(),
                plateau,
            }),
            HeightJson::Provider(HeightDistribution::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            }) => HeightProvider::BiasedToBottom(BiasedToBottomHeightProvider {
                min_inclusive: min_inclusive.into_offset(),
                max_inclusive: max_inclusive.into_offset(),
                inner,
            }),
            HeightJson::Provider(HeightDistribution::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            }) => HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeightProvider {
                min_inclusive: min_inclusive.into_offset(),
                max_inclusive: max_inclusive.into_offset(),
                inner,
            }),
        };
        let additional = match self.kind {
            Kind::Cave(cave) => CarverAdditionalConfig::Cave(CaveCarverConfig {
                count: cave.count,
                horizontal_radius_multiplier: cave.horizontal_radius_multiplier,
                vertical_radius_multiplier: cave.vertical_radius_multiplier,
                floor_level: cave.floor_level,
                room_vertical_radius_multiplier: cave.room_vertical_radius_multiplier,
                start_vertical_radius_multiplier: cave.start_vertical_radius_multiplier,
                thickness: cave.thickness,
                weird_thickness_bias: cave.weird_thickness_bias,
            }),
            Kind::Canyon(canyon) => CarverAdditionalConfig::Canyon(CanyonCarverConfig {
                vertical_rotation: canyon.vertical_rotation,
                shape: CanyonShapeConfig {
                    distance_factor: canyon.shape.distance_factor,
                    thickness: canyon.shape.thickness,
                    width_smoothness: canyon.shape.width_smoothness,
                    horizontal_radius_factor: canyon.shape.horizontal_radius_factor,
                    vertical_radius_default_factor: canyon.shape.vertical_radius_default_factor,
                    vertical_radius_center_factor: canyon.shape.vertical_radius_center_factor,
                    y_scale: canyon.shape.y_scale,
                },
            }),
        };
        CarverConfig {
            registry_id,
            y,
            additional,
            probability: self.probability,
        }
    }
}
