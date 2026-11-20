use std::collections::BTreeMap;

use super::{
    Axis, BinaryOperation, DensityFunctionRepr, DistanceMetric, LinearOperation, NoiseRouterRepr,
    RoundingOperation, RouterRoots, SplineRepr, Tiling, UnaryOperation, WrapperType,
};
use pumpkin_data::{chunk::DoublePerlinNoiseParameters, noise_router as runtime};
use runtime::{
    BaseMultiNoiseRouter, BaseNoiseFunctionComponent, BaseNoiseRouter, BaseNoiseRouters,
    BaseSurfaceEstimator, BaseVeinRouter, SplinePoint,
};

struct RuntimeStack<'a> {
    noise: &'a dyn Fn(&str) -> Result<DoublePerlinNoiseParameters, String>,
    components: Vec<BaseNoiseFunctionComponent>,
    indices: BTreeMap<u64, usize>,
}

impl<'a> RuntimeStack<'a> {
    fn new(noise: &'a dyn Fn(&str) -> Result<DoublePerlinNoiseParameters, String>) -> Self {
        Self {
            noise,
            components: Vec::new(),
            indices: BTreeMap::new(),
        }
    }

    fn finish(self) -> &'static [BaseNoiseFunctionComponent] {
        Box::leak(self.components.into_boxed_slice())
    }
}
impl NoiseRouterRepr {
    /// Builds a router once, using the supplied merged noise registry.
    pub fn build_runtime(
        mut self,
        noise_parameters: impl Fn(&str) -> Result<DoublePerlinNoiseParameters, String>,
    ) -> Result<BaseNoiseRouters, String> {
        self.optimize();
        self.slice_uniform_axes();
        let mut noise = RuntimeStack::new(&noise_parameters);
        let mut surface = RuntimeStack::new(&noise_parameters);
        let mut multinoise = RuntimeStack::new(&noise_parameters);
        let mut vein = RuntimeStack::new(&noise_parameters);
        let RouterRoots {
            noise_erosion,
            noise_depth,
            barrier_noise,
            fluid_level_floodedness_noise,
            fluid_level_spread_noise,
            lava_noise,
            final_density,
            vein_toggle,
            vein_ridged,
            vein_gap,
            ridges,
            temperature,
            vegetation,
            continents,
            multi_erosion,
            multi_depth,
            vein_router_toggle,
            vein_router_ridged,
            vein_router_gap,
        } = self.visit_roots(
            [&mut noise, &mut surface, &mut multinoise, &mut vein],
            DensityFunctionRepr::build_index,
        )?;
        Ok(BaseNoiseRouters {
            noise: BaseNoiseRouter {
                full_component_stack: noise.finish(),
                barrier_noise,
                fluid_level_floodedness_noise,
                fluid_level_spread_noise,
                lava_noise,
                erosion: noise_erosion,
                depth: noise_depth,
                final_density,
                vein_toggle,
                vein_ridged,
                vein_gap,
            },
            surface_estimator: BaseSurfaceEstimator {
                full_component_stack: surface.finish(),
            },
            multi_noise: BaseMultiNoiseRouter {
                full_component_stack: multinoise.finish(),
                temperature,
                vegetation,
                continents,
                erosion: multi_erosion,
                depth: multi_depth,
                ridges,
            },
            vein: BaseVeinRouter {
                full_component_stack: vein.finish(),
                vein_toggle: vein_router_toggle,
                vein_ridged: vein_router_ridged,
                vein_gap: vein_router_gap,
            },
        })
    }
}

impl SplineRepr {
    fn build_component(&self, stack: &mut RuntimeStack<'_>) -> Result<runtime::SplineRepr, String> {
        Ok(match self {
            Self::Fixed { value } => runtime::SplineRepr::Fixed { value: value.0 },
            Self::Standard {
                location_function,
                locations,
                values,
                derivatives,
            } => {
                let points = locations
                    .into_iter()
                    .zip(values)
                    .zip(derivatives)
                    .map(|((location, value), derivative)| (location, value, derivative));

                let function_index = location_function.build_index(stack)?;

                let point_reprs = points
                    .into_iter()
                    .map(|(location, value, derivative)| {
                        let value_repr = value.build_component(stack)?;

                        Ok(SplinePoint {
                            location: location.0,
                            value: Box::leak(Box::new(value_repr)),
                            derivative: derivative.0,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                runtime::SplineRepr::Standard {
                    location_function_index: function_index,
                    points: Box::leak(point_reprs.into_boxed_slice()),
                }
            }
        })
    }
}
impl DensityFunctionRepr {
    fn build_index(&self, stack: &mut RuntimeStack<'_>) -> Result<usize, String> {
        if let Some(index) = stack.indices.get(&self.unique_id()) {
            Ok(*index)
        } else {
            let id = self.unique_id();
            let repr = self.build_component(stack)?;
            stack.components.push(repr);
            let index = stack.components.len() - 1;
            stack.indices.insert(id, index);
            Ok(index)
        }
    }
    #[expect(clippy::too_many_lines, reason = "one component per density node")]
    fn build_component(
        &self,
        stack: &mut RuntimeStack<'_>,
    ) -> Result<BaseNoiseFunctionComponent, String> {
        Ok(match self {
            Self::Spline { spline, data } => {
                let _ = data;
                let spline_repr = spline.build_component(stack)?;

                BaseNoiseFunctionComponent::Spline {
                    spline: Box::leak(Box::new(spline_repr)),
                }
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                data,
            } => {
                let density_index = density.build_index(stack)?;
                let upper_bound_index = upper_bound.build_index(stack)?;
                let lower_bound = data.lower_bound;
                let cell_height = data.cell_height;

                BaseNoiseFunctionComponent::FindTopSurface {
                    density_index,
                    upper_bound_index,
                    data: Box::leak(Box::new(runtime::FindTopSurfaceData {
                        lower_bound,
                        cell_height,
                    })),
                }
            }
            Self::EndIslands => BaseNoiseFunctionComponent::EndIslands,
            Self::Noise { data } => {
                let noise_id = (stack.noise)(&data.noise_id)?;
                let xz_scale = data.xz_scale.0;
                let y_scale = data.y_scale.0;

                BaseNoiseFunctionComponent::Noise {
                    data: Box::leak(Box::new(runtime::NoiseData {
                        noise_id,
                        xz_scale,
                        y_scale,
                    })),
                }
            }
            Self::ShiftA { noise_id } => {
                let noise_id = (stack.noise)(noise_id)?;

                BaseNoiseFunctionComponent::ShiftA { noise_id }
            }
            Self::ShiftB { noise_id } => {
                let noise_id = (stack.noise)(noise_id)?;

                BaseNoiseFunctionComponent::ShiftB { noise_id }
            }
            Self::BlendDensity { input } => {
                let input_index = input.build_index(stack)?;

                BaseNoiseFunctionComponent::BlendDensity { input_index }
            }
            Self::BlendAlpha => BaseNoiseFunctionComponent::BlendAlpha,
            Self::BlendOffset => BaseNoiseFunctionComponent::BlendOffset,
            Self::Beardifier => BaseNoiseFunctionComponent::Beardifier,
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => {
                let shift_x_index = shift_x.build_index(stack)?;
                let shift_y_index = shift_y.build_index(stack)?;
                let shift_z_index = shift_z.build_index(stack)?;

                let xz_scale = data.xz_scale.0;
                let y_scale = data.y_scale.0;
                let noise_id = (stack.noise)(&data.noise_id)?;

                BaseNoiseFunctionComponent::ShiftedNoise {
                    shift_x_index,
                    shift_y_index,
                    shift_z_index,
                    data: Box::leak(Box::new(runtime::ShiftedNoiseData {
                        xz_scale,
                        y_scale,
                        noise_id,
                    })),
                }
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                data,
            } => {
                let input_index = input.build_index(stack)?;
                let when_in_index = when_in_range.build_index(stack)?;
                let when_out_index = when_out_range.build_index(stack)?;

                let min_inclusive = data.min_inclusive.0;
                let max_exclusive = data.max_exclusive.0;

                BaseNoiseFunctionComponent::RangeChoice {
                    input_index,
                    when_in_range_index: when_in_index,
                    when_out_range_index: when_out_index,
                    data: Box::leak(Box::new(runtime::RangeChoiceData {
                        min_inclusive,
                        max_exclusive,
                    })),
                }
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => {
                let argument1_index = argument1.build_index(stack)?;
                let argument2_index = argument2.build_index(stack)?;

                let action = data.operation.into();
                BaseNoiseFunctionComponent::Binary {
                    argument1_index,
                    argument2_index,
                    data: Box::leak(Box::new(runtime::BinaryData { operation: action })),
                }
            }
            Self::ClampedYGradient { data } => {
                let from_y = data.from_y as f32;
                let to_y = data.to_y as f32;
                let from_value = data.from_value.0;
                let to_value = data.to_value.0;

                BaseNoiseFunctionComponent::ClampedYGradient {
                    data: Box::leak(Box::new(runtime::ClampedYGradientData {
                        from_y,
                        to_y,
                        from_value,
                        to_value,
                    })),
                }
            }
            Self::Gradient { data } => {
                let axis = data.axis.into();
                let tiling = data.tiling.into();
                let from_coordinate = data.from_coordinate;
                let to_coordinate = data.to_coordinate;
                let from_value = data.from_value.0;
                let to_value = data.to_value.0;

                BaseNoiseFunctionComponent::Gradient {
                    data: Box::leak(Box::new(runtime::GradientData {
                        axis,
                        tiling,
                        from_coordinate,
                        to_coordinate,
                        from_value,
                        to_value,
                    })),
                }
            }
            Self::DistanceToPoint { data } => {
                let px = data.point[0];
                let py = data.point[1];
                let pz = data.point[2];
                let metric = data.metric.into();

                BaseNoiseFunctionComponent::DistanceToPoint {
                    data: Box::leak(Box::new(runtime::DistanceToPointData {
                        point: [px, py, pz],
                        metric,
                    })),
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha_index = alpha.build_index(stack)?;
                let first_index = first.build_index(stack)?;
                let second_index = second.build_index(stack)?;

                BaseNoiseFunctionComponent::Lerp {
                    alpha_index,
                    first_index,
                    second_index,
                }
            }
            Self::Rounding {
                input,
                multiple,
                data,
            } => {
                let input_index = input.build_index(stack)?;
                let multiple_index = multiple.build_index(stack)?;
                let action = data.operation.into();

                BaseNoiseFunctionComponent::Rounding {
                    input_index,
                    multiple_index,
                    data: Box::leak(Box::new(runtime::RoundingData { operation: action })),
                }
            }
            Self::Slice {
                axis,
                coordinate,
                input,
            } => {
                let input_index = input.build_index(stack)?;
                let axis_ts = (*axis).into();

                BaseNoiseFunctionComponent::Slice {
                    input_index,
                    axis: axis_ts,
                    coordinate: *coordinate,
                }
            }
            Self::Constant { value } => BaseNoiseFunctionComponent::Constant { value: value.0 },
            Self::Wrapper { input, wrapper } => {
                let input_index = input.build_index(stack)?;
                let wrapper_repr = (*wrapper).into();

                BaseNoiseFunctionComponent::Wrapper {
                    input_index,
                    wrapper: wrapper_repr,
                }
            }
            Self::Linear { input, data } => {
                let input_index = input.build_index(stack)?;

                let action = data.operation.into();
                let argument = data.argument.0;
                BaseNoiseFunctionComponent::Linear {
                    input_index,
                    data: Box::leak(Box::new(runtime::LinearData {
                        operation: action,
                        argument,
                    })),
                }
            }
            Self::Clamp { input, data } => {
                let input_index = input.build_index(stack)?;

                let min_value = data.min_value.0;
                let max_value = data.max_value.0;

                BaseNoiseFunctionComponent::Clamp {
                    input_index,
                    data: Box::leak(Box::new(runtime::ClampData {
                        min_value,
                        max_value,
                    })),
                }
            }
            Self::Unary { input, data } => {
                let input_index = input.build_index(stack)?;

                let action = data.operation.into();

                BaseNoiseFunctionComponent::Unary {
                    input_index,
                    data: Box::leak(Box::new(runtime::UnaryData { operation: action })),
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let input_index = input.build_index(stack)?;
                let functions_indices = functions
                    .iter()
                    .map(|f| f.build_index(stack))
                    .collect::<Result<Vec<_>, _>>()?;
                let thresholds = thresholds.iter().map(|t| t.0).collect::<Vec<_>>();

                BaseNoiseFunctionComponent::IntervalSelect {
                    input_index,
                    thresholds: Box::leak(thresholds.into_boxed_slice()),
                    functions_indices: Box::leak(functions_indices.into_boxed_slice()),
                }
            }
            Self::InterpolatedNoiseSampler { data } => {
                let xz_scale = data.xz_scale.0;
                let y_scale = data.y_scale.0;
                let xz_factor = data.xz_factor.0;
                let y_factor = data.y_factor.0;
                let smear_scale_multiplier = data.smear_scale_multiplier.0;

                BaseNoiseFunctionComponent::InterpolatedNoiseSampler {
                    data: Box::leak(Box::new(runtime::InterpolatedNoiseSamplerData {
                        xz_scale,
                        y_scale,
                        xz_factor,
                        y_factor,
                        smear_scale_multiplier,
                    })),
                }
            }
        })
    }
}

impl From<BinaryOperation> for runtime::BinaryOperation {
    fn from(value: BinaryOperation) -> Self {
        match value {
            BinaryOperation::Add => Self::Add,
            BinaryOperation::Mul => Self::Mul,
            BinaryOperation::Min => Self::Min,
            BinaryOperation::Max => Self::Max,
            BinaryOperation::Sub => Self::Sub,
            BinaryOperation::Div => Self::Div,
            BinaryOperation::Pow => Self::Pow,
        }
    }
}

impl From<LinearOperation> for runtime::LinearOperation {
    fn from(value: LinearOperation) -> Self {
        match value {
            LinearOperation::Add => Self::Add,
            LinearOperation::Mul => Self::Mul,
        }
    }
}

impl From<UnaryOperation> for runtime::UnaryOperation {
    fn from(value: UnaryOperation) -> Self {
        match value {
            UnaryOperation::Invert => Self::Invert,
            UnaryOperation::Abs => Self::Abs,
            UnaryOperation::Square => Self::Square,
            UnaryOperation::Cube => Self::Cube,
            UnaryOperation::HalfNegative => Self::HalfNegative,
            UnaryOperation::QuarterNegative => Self::QuarterNegative,
            UnaryOperation::Squeeze => Self::Squeeze,
            UnaryOperation::Negate => Self::Negate,
            UnaryOperation::Sqrt => Self::Sqrt,
            UnaryOperation::Log => Self::Log,
            UnaryOperation::Sign => Self::Sign,
        }
    }
}

impl From<RoundingOperation> for runtime::RoundingOperation {
    fn from(value: RoundingOperation) -> Self {
        match value {
            RoundingOperation::Floor => Self::Floor,
            RoundingOperation::Round => Self::Round,
            RoundingOperation::Ceil => Self::Ceil,
            RoundingOperation::Truncate => Self::Truncate,
        }
    }
}

impl From<DistanceMetric> for runtime::DistanceMetric {
    fn from(value: DistanceMetric) -> Self {
        match value {
            DistanceMetric::Euclidean => Self::Euclidean,
            DistanceMetric::EuclideanSquared => Self::EuclideanSquared,
            DistanceMetric::Manhattan => Self::Manhattan,
            DistanceMetric::Chebyshev => Self::Chebyshev,
        }
    }
}

impl From<Axis> for runtime::Axis {
    fn from(value: Axis) -> Self {
        match value {
            Axis::X => Self::X,
            Axis::Y => Self::Y,
            Axis::Z => Self::Z,
        }
    }
}

impl From<Tiling> for runtime::Tiling {
    fn from(value: Tiling) -> Self {
        match value {
            Tiling::ClampToEdge => Self::ClampToEdge,
            Tiling::Repeat => Self::Repeat,
            Tiling::MirroredRepeat => Self::MirroredRepeat,
        }
    }
}

impl From<WrapperType> for runtime::WrapperType {
    fn from(value: WrapperType) -> Self {
        match value {
            WrapperType::Cache => Self::Cache,
            WrapperType::Interpolated {
                cell_size_xz,
                cell_size_y,
            } => Self::Interpolated {
                cell_size_xz,
                cell_size_y,
            },
        }
    }
}
