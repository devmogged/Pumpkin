use std::{collections::BTreeMap, fs, path::Path};

use proc_macro2::TokenStream;
use pumpkin_worldgen_model::noise_router::parse_noise_router;
use quote::quote;
use serde_json::Value;

fn collect_densities(dir: &Path, prefix: &str, values: &mut BTreeMap<String, Value>) {
    for entry in fs::read_dir(dir).expect("read density functions") {
        let entry = entry.expect("read density entry");
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().expect("UTF-8 density function path");
        if path.is_dir() {
            collect_densities(&path, &format!("{prefix}{name}/"), values);
        } else if let Some(name) = name.strip_suffix(".json") {
            let json = fs::read_to_string(&path).expect("read density function");
            let value = serde_json::from_str(&json).expect("parse density function JSON");
            values.insert(format!("{prefix}{name}"), value);
        }
    }
}

pub fn build() -> TokenStream {
    let worldgen = Path::new("../../assets/datapack/data/minecraft/worldgen");
    let mut densities = BTreeMap::new();
    collect_densities(
        &worldgen.join("density_function"),
        "minecraft:",
        &mut densities,
    );
    let density_sources = densities.keys().map(|id| {
        let file = id.trim_start_matches("minecraft:");
        let path = format!(
            "../../../../assets/datapack/data/minecraft/worldgen/density_function/{file}.json"
        );
        quote!((#id, include_str!(#path)))
    });
    let compile = |name: &str| {
        let path = worldgen.join("noise_settings").join(format!("{name}.json"));
        let json = fs::read_to_string(path).expect("read noise settings");
        let value = serde_json::from_str(&json).expect("parse noise settings JSON");
        parse_noise_router(name, &value, &densities)
            .unwrap_or_else(|error| panic!("noise settings {name}: {error}"))
            .into_token_stream_compiled(name)
    };
    let (overworld_router, overworld_compiled) = compile("overworld");
    let (amplified_router, amplified_compiled) = compile("amplified");
    let (large_biomes_router, large_biomes_compiled) = compile("large_biomes");
    let (nether_router, nether_compiled) = compile("nether");
    let (end_router, end_compiled) = compile("end");

    quote! {
        use crate::chunk::DoublePerlinNoiseParameters;

        pub const VANILLA_DENSITY_JSON: &[(&str, &str)] = &[#(#density_sources),*];

        pub trait NoiseEvaluationContext {
            fn sample_noise(&mut self, noise_id: DoublePerlinNoiseParameters, x: f64, y: f64, z: f64) -> f32;
            fn sample_shift_a(&mut self, noise_id: DoublePerlinNoiseParameters, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_shift_b(&mut self, noise_id: DoublePerlinNoiseParameters, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_shifted_noise(&mut self, noise_id: DoublePerlinNoiseParameters, shift_x: f32, shift_y: f32, shift_z: f32, xz_scale: f64, y_scale: f64) -> f32;
            fn sample_interpolated_noise(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_beardifier(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_blend_alpha(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_blend_offset(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_blend_density(&mut self, input_val: f32, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_end_islands(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_wrapper(&mut self, wrapper_index: usize, wrapper_type: WrapperType, pos: &pumpkin_util::math::vector3::Vector3<i32>, eval_input: &dyn Fn(&pumpkin_util::math::vector3::Vector3<i32>, &mut Self) -> f32) -> f32;
            fn sample_spline(&mut self, spline_index: usize, location_value: f32, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_find_top_surface(&mut self, density_fn: &dyn Fn(&pumpkin_util::math::vector3::Vector3<i32>, &mut Self) -> f32, upper_bound_fn: &dyn Fn(&pumpkin_util::math::vector3::Vector3<i32>, &mut Self) -> f32, lower_bound: i32, cell_height: i32, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
        }

        #overworld_compiled
        #amplified_compiled
        #large_biomes_compiled
        #nether_compiled
        #end_compiled

        #[derive(Debug, PartialEq)]
        pub struct NoiseData {
            pub noise_id: DoublePerlinNoiseParameters,
            pub xz_scale: f64,
            pub y_scale: f64,
        }

        #[derive(Debug, PartialEq)]
        pub struct FindTopSurfaceData {
            pub lower_bound: i32,
            pub cell_height: i32,
        }

        #[derive(Debug, PartialEq)]
        pub struct ShiftedNoiseData {
            pub xz_scale: f64,
            pub y_scale: f64,
            pub noise_id: DoublePerlinNoiseParameters,
        }

        #[derive(Debug, PartialEq)]
        pub struct InterpolatedNoiseSamplerData {
            pub xz_scale: f64,
            pub y_scale: f64,
            pub xz_factor: f64,
            pub y_factor: f64,
            pub smear_scale_multiplier: f64,
        }

        #[derive(Debug, PartialEq)]
        pub struct ClampedYGradientData {
            pub from_y: f32,
            pub to_y: f32,
            pub from_value: f32,
            pub to_value: f32,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum Axis {
            X,
            Y,
            Z,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum Tiling {
            ClampToEdge,
            Repeat,
            MirroredRepeat,
        }

        #[derive(Debug, PartialEq)]
        pub struct GradientData {
            pub axis: Axis,
            pub tiling: Tiling,
            pub from_coordinate: i32,
            pub to_coordinate: i32,
            pub from_value: f32,
            pub to_value: f32,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum DistanceMetric {
            Euclidean,
            EuclideanSquared,
            Manhattan,
            Chebyshev,
        }

        #[derive(Debug, PartialEq)]
        pub struct DistanceToPointData {
            pub point: [i32; 3],
            pub metric: DistanceMetric,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum RoundingOperation {
            Floor,
            Round,
            Ceil,
            Truncate,
        }

        #[derive(Debug, PartialEq)]
        pub struct RoundingData {
            pub operation: RoundingOperation,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum BinaryOperation {
            Add,
            Mul,
            Min,
            Max,
            Sub,
            Div,
            Pow,
        }

        #[derive(Debug, PartialEq)]
        pub struct BinaryData {
            pub operation: BinaryOperation,
        }

        impl BinaryData {
            #[inline]
            #[must_use]
            pub const fn apply_density(&self, a: f32, b: f32) -> f32 {
                match self.operation {
                    BinaryOperation::Add => a + b,
                    BinaryOperation::Mul => a * b,
                    BinaryOperation::Min => a.min(b),
                    BinaryOperation::Max => a.max(b),
                    BinaryOperation::Sub => a - b,
                    BinaryOperation::Div => if b == 0.0 { 0.0 } else { a / b },
                    BinaryOperation::Pow => a, // const evaluation fallback
                }
            }
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum LinearOperation {
            Add,
            Mul,
        }

        #[derive(Debug, PartialEq)]
        pub struct LinearData {
            pub operation: LinearOperation,
            pub argument: f32,
        }

        impl LinearData {
            #[inline]
            #[must_use]
            pub const fn apply_density(&self, density: f32) -> f32 {
                match self.operation {
                    LinearOperation::Add => density + self.argument,
                    LinearOperation::Mul => density * self.argument,
                }
            }
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub enum UnaryOperation {
            Abs,
            Square,
            Cube,
            HalfNegative,
            QuarterNegative,
            Squeeze,
            Invert,
            Negate,
            Sqrt,
            Log,
            Sign,
        }

        #[derive(Debug, PartialEq)]
        pub struct UnaryData {
            pub operation: UnaryOperation,
        }

        impl UnaryData {
            #[inline]
            #[must_use]
            pub fn apply_density(&self, density: f32) -> f32 {
                match self.operation {
                    UnaryOperation::Abs => density.abs(),
                    UnaryOperation::Square => density * density,
                    UnaryOperation::Cube => density * density * density,
                    UnaryOperation::HalfNegative => {
                        if density > 0.0 {
                            density
                        } else {
                            density * 0.5
                        }
                    }
                    UnaryOperation::QuarterNegative => {
                        if density > 0.0 {
                            density
                        } else {
                            density * 0.25
                        }
                    }
                    UnaryOperation::Squeeze => {
                        let clamped = density.clamp(-1.0, 1.0);
                        clamped / 2.0 - clamped * clamped * clamped / 24.0
                    }
                    UnaryOperation::Invert => {
                        if density == 0.0 { f32::INFINITY } else { 1.0 / density }
                    }
                    UnaryOperation::Negate => -density,
                    UnaryOperation::Sqrt => density.sqrt(),
                    UnaryOperation::Log => density.ln(),
                    UnaryOperation::Sign => {
                        if density > 0.0 { 1.0 } else if density < 0.0 { -1.0 } else { 0.0 }
                    }
                }
            }
        }

        #[derive(Debug, PartialEq)]
        pub struct ClampData {
            pub min_value: f32,
            pub max_value: f32,
        }

        impl ClampData {
            #[inline]
            #[must_use]
            pub const fn apply_density(&self, density: f32) -> f32 {
                density.clamp(self.min_value, self.max_value)
            }
        }

        #[derive(Debug, PartialEq)]
        pub struct RangeChoiceData {
            pub min_inclusive: f32,
            pub max_exclusive: f32,
        }

        #[derive(Debug, PartialEq)]
        pub struct SplinePoint {
            pub location: f32,
            pub value: &'static SplineRepr,
            pub derivative: f32,
        }

        #[derive(Debug, PartialEq)]
        pub enum SplineRepr {
            Standard {
                location_function_index: usize,
                points: &'static [SplinePoint],
            },
            Fixed { value: f32 },
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum WrapperType {
            Interpolated { cell_size_xz: i32, cell_size_y: i32 },
            Cache,
        }

        #[derive(Debug, PartialEq)]
        pub enum BaseNoiseFunctionComponent {
            Beardifier,
            BlendAlpha,
            BlendOffset,
            BlendDensity {
                input_index: usize,
            },
            FindTopSurface {
                density_index: usize,
                upper_bound_index: usize,
                data: &'static FindTopSurfaceData,
            },
            EndIslands,
            Noise {
                data: &'static NoiseData,
            },
            ShiftA {
                noise_id: DoublePerlinNoiseParameters,
            },
            ShiftB {
                noise_id: DoublePerlinNoiseParameters,
            },
            ShiftedNoise {
                shift_x_index: usize,
                shift_y_index: usize,
                shift_z_index: usize,
                data: &'static ShiftedNoiseData,
            },
            InterpolatedNoiseSampler {
                data: &'static InterpolatedNoiseSamplerData,
            },
            IntervalSelect {
                input_index: usize,
                thresholds: &'static [f32],
                functions_indices: &'static [usize],
            },
            Wrapper {
                input_index: usize,
                wrapper: WrapperType,
            },
            Constant {
                value: f32,
            },
            ClampedYGradient {
                data: &'static ClampedYGradientData,
            },
            Gradient {
                data: &'static GradientData,
            },
            DistanceToPoint {
                data: &'static DistanceToPointData,
            },
            Lerp {
                alpha_index: usize,
                first_index: usize,
                second_index: usize,
            },
            Rounding {
                input_index: usize,
                multiple_index: usize,
                data: &'static RoundingData,
            },
            Slice {
                input_index: usize,
                axis: Axis,
                coordinate: i32,
            },
            Binary {
                argument1_index: usize,
                argument2_index: usize,
                data: &'static BinaryData,
            },
            Linear {
                input_index: usize,
                data: &'static LinearData,
            },
            Unary {
                input_index: usize,
                data: &'static UnaryData,
            },
            Clamp {
                input_index: usize,
                data: &'static ClampData,
            },
            RangeChoice {
                input_index: usize,
                when_in_range_index: usize,
                when_out_range_index: usize,
                data: &'static RangeChoiceData,
            },
            Spline {
                spline: &'static SplineRepr,
            },
        }

        #[derive(Debug, PartialEq)]
        pub struct BaseNoiseRouter {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
            pub barrier_noise: usize,
            pub fluid_level_floodedness_noise: usize,
            pub fluid_level_spread_noise: usize,
            pub lava_noise: usize,
            pub erosion: usize,
            pub depth: usize,
            pub final_density: usize,
            pub vein_toggle: usize,
            pub vein_ridged: usize,
            pub vein_gap: usize,
        }

        #[derive(Debug, PartialEq)]
        pub struct BaseSurfaceEstimator {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
        }

        #[derive(Debug, PartialEq)]
        pub struct BaseMultiNoiseRouter {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
            pub temperature: usize,
            pub vegetation: usize,
            pub continents: usize,
            pub erosion: usize,
            pub depth: usize,
            pub ridges: usize,
        }

        #[derive(Debug, PartialEq)]
        pub struct BaseVeinRouter {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
            pub vein_toggle: usize,
            pub vein_ridged: usize,
            pub vein_gap: usize,
        }

        #[derive(Debug, PartialEq)]
        pub struct BaseNoiseRouters {
            pub noise: BaseNoiseRouter,
            pub surface_estimator: BaseSurfaceEstimator,
            pub multi_noise: BaseMultiNoiseRouter,
            pub vein: BaseVeinRouter,
        }

        pub const OVERWORLD_BASE_NOISE_ROUTER: BaseNoiseRouters = #overworld_router;
        pub const AMPLIFIED_BASE_NOISE_ROUTER: BaseNoiseRouters = #amplified_router;
        pub const LARGE_BIOMES_BASE_NOISE_ROUTER: BaseNoiseRouters = #large_biomes_router;
        pub const NETHER_BASE_NOISE_ROUTER: BaseNoiseRouters = #nether_router;
        pub const END_BASE_NOISE_ROUTER: BaseNoiseRouters = #end_router;

        impl BaseNoiseRouters {
            #[must_use]
            pub fn from_name(name: &str) -> Option<&'static Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                match name {
                    "overworld" => Some(&OVERWORLD_BASE_NOISE_ROUTER),
                    "amplified" => Some(&AMPLIFIED_BASE_NOISE_ROUTER),
                    "large_biomes" => Some(&LARGE_BIOMES_BASE_NOISE_ROUTER),
                    "nether" => Some(&NETHER_BASE_NOISE_ROUTER),
                    "end" => Some(&END_BASE_NOISE_ROUTER),
                    _ => None,
                }
            }

            #[must_use]
            pub fn from_dimension(dimension: &crate::dimension::Dimension) -> &'static Self {
                if dimension == &crate::dimension::Dimension::OVERWORLD {
                    &OVERWORLD_BASE_NOISE_ROUTER
                } else if dimension == &crate::dimension::Dimension::THE_NETHER {
                    &NETHER_BASE_NOISE_ROUTER
                } else {
                    &END_BASE_NOISE_ROUTER
                }
            }
        }
    }
}
