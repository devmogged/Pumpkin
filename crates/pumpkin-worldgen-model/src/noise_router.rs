use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
};

#[cfg(feature = "codegen")]
use heck::ToShoutySnakeCase;

#[cfg(feature = "codegen")]
use proc_macro2::{Punct, Spacing, Span, TokenStream};
#[cfg(feature = "codegen")]
use quote::{ToTokens, TokenStreamExt, quote};
use serde::Deserialize;
use serde_json::Value;
#[cfg(feature = "codegen")]
use syn::Ident;

/// Wraps an `f32` to provide a bitwise-exact `Hash` implementation for use as a map key.
#[derive(Clone, Copy)]
struct HashableF32(pub f32);

impl Hash for HashableF32 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_le_bytes().hash(state);
    }
}

#[cfg(feature = "codegen")]
impl ToTokens for HashableF32 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let value = self.0;
        if value.is_finite() {
            value.to_tokens(tokens);
        } else {
            tokens.append(Ident::new("f32", Span::call_site()));
            tokens.append(Punct::new(':', Spacing::Joint));
            tokens.append(Punct::new(':', Spacing::Joint));
            if value.is_nan() {
                tokens.append(Ident::new("NAN", Span::call_site()));
            } else if value > 0.0 {
                tokens.append(Ident::new("INFINITY", Span::call_site()));
            } else {
                tokens.append(Ident::new("NEG_INFINITY", Span::call_site()));
            }
        }
    }
}

impl<'de> Deserialize<'de> for HashableF32 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self)
    }
}

#[derive(Clone, Copy)]
struct HashableF64(pub f64);

impl Hash for HashableF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_le_bytes().hash(state);
    }
}

#[cfg(feature = "codegen")]
impl ToTokens for HashableF64 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.to_tokens(tokens);
    }
}

impl<'de> Deserialize<'de> for HashableF64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f64::deserialize(deserializer).map(Self)
    }
}

/// Deserialized representation of a cubic spline used inside density functions.
#[derive(Deserialize, Hash, Clone)]
#[serde(tag = "_type", content = "value")]
enum SplineRepr {
    #[serde(rename(deserialize = "standard"))]
    Standard {
        #[serde(rename(deserialize = "locationFunction"))]
        location_function: Box<DensityFunctionRepr>,
        locations: Box<[HashableF32]>,
        values: Box<[Self]>,
        derivatives: Box<[HashableF32]>,
    },
    #[serde(rename(deserialize = "fixed"))]
    Fixed { value: HashableF32 },
}

impl SplineRepr {
    #[cfg(feature = "codegen")]
    fn get_token_stream(
        &self,
        stack: &mut Vec<TokenStream>,
        nodes: &mut Vec<DensityFunctionRepr>,
        hash_to_index_map: &mut BTreeMap<u64, usize>,
    ) -> TokenStream {
        match self {
            Self::Fixed { value } => {
                quote! {
                    SplineRepr::Fixed { value: #value }
                }
            }
            Self::Standard {
                location_function,
                locations,
                values,
                derivatives,
            } => {
                assert_eq!(values.len(), locations.len());
                assert_eq!(values.len(), derivatives.len());

                let points = locations
                    .into_iter()
                    .zip(values)
                    .zip(derivatives)
                    .map(|((location, value), derivative)| (location, value, derivative));

                let function_index =
                    location_function.get_index_for_component(stack, nodes, hash_to_index_map);

                let point_reprs = points
                    .into_iter()
                    .map(|(location, value, derivative)| {
                        let value_repr = value.get_token_stream(stack, nodes, hash_to_index_map);

                        quote! {
                            SplinePoint {
                                location: #location,
                                value: &#value_repr,
                                derivative: #derivative,
                            }
                        }
                    })
                    .collect::<Vec<_>>();

                quote! {
                    SplineRepr::Standard {
                        location_function_index: #function_index,
                        points: &[#(#point_reprs),*],
                    }
                }
            }
        }
    }
}

/// Arithmetic operation applied to two density function arguments.
#[derive(Deserialize, Hash, Copy, Clone)]
enum BinaryOperation {
    #[serde(rename(deserialize = "ADD"))]
    Add,
    #[serde(rename(deserialize = "MUL"))]
    Mul,
    #[serde(rename(deserialize = "MIN"))]
    Min,
    #[serde(rename(deserialize = "MAX"))]
    Max,
    Sub,
    Div,
    Pow,
}

impl BinaryOperation {
    #[cfg(feature = "codegen")]
    fn get_token_stream(self) -> TokenStream {
        match self {
            Self::Add => quote! { BinaryOperation::Add },
            Self::Mul => quote! { BinaryOperation::Mul },
            Self::Min => quote! { BinaryOperation::Min },
            Self::Max => quote! { BinaryOperation::Max },
            Self::Sub => quote! { BinaryOperation::Sub },
            Self::Div => quote! { BinaryOperation::Div },
            Self::Pow => quote! { BinaryOperation::Pow },
        }
    }
}

/// Arithmetic operation applied to a single density function argument and a scalar.
#[derive(Deserialize, Hash, Copy, Clone)]
enum LinearOperation {
    #[serde(rename(deserialize = "ADD"))]
    Add,
    #[serde(rename(deserialize = "MUL"))]
    Mul,
}

impl LinearOperation {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Add => quote! { LinearOperation::Add },
            Self::Mul => quote! { LinearOperation::Mul },
        }
    }
}

/// Single-argument transformation applied to a density value.
#[derive(Deserialize, Hash, Copy, Clone)]
enum UnaryOperation {
    #[serde(rename(deserialize = "INVERT"))]
    Invert,
    #[serde(rename(deserialize = "ABS"))]
    Abs,
    #[serde(rename(deserialize = "SQUARE"))]
    Square,
    #[serde(rename(deserialize = "CUBE"))]
    Cube,
    #[serde(rename(deserialize = "HALF_NEGATIVE"))]
    HalfNegative,
    #[serde(rename(deserialize = "QUARTER_NEGATIVE"))]
    QuarterNegative,
    #[serde(rename(deserialize = "SQUEEZE"))]
    Squeeze,
    Negate,
    Sqrt,
    Log,
    Sign,
}

impl UnaryOperation {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Invert => quote! { UnaryOperation::Invert },
            Self::Abs => quote! { UnaryOperation::Abs },
            Self::Square => quote! { UnaryOperation::Square },
            Self::Cube => quote! { UnaryOperation::Cube },
            Self::HalfNegative => quote! { UnaryOperation::HalfNegative },
            Self::QuarterNegative => quote! { UnaryOperation::QuarterNegative },
            Self::Squeeze => quote! { UnaryOperation::Squeeze },
            Self::Negate => quote! { UnaryOperation::Negate },
            Self::Sqrt => quote! { UnaryOperation::Sqrt },
            Self::Log => quote! { UnaryOperation::Log },
            Self::Sign => quote! { UnaryOperation::Sign },
        }
    }
}

/// Rounding operations.
#[derive(Deserialize, Hash, Copy, Clone)]
enum RoundingOperation {
    Floor,
    Round,
    Ceil,
    Truncate,
}

impl RoundingOperation {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Floor => quote! { RoundingOperation::Floor },
            Self::Round => quote! { RoundingOperation::Round },
            Self::Ceil => quote! { RoundingOperation::Ceil },
            Self::Truncate => quote! { RoundingOperation::Truncate },
        }
    }
}

/// Distance metric for `distance_to_point`.
#[derive(Deserialize, Hash, Copy, Clone)]
enum DistanceMetric {
    Euclidean,
    EuclideanSquared,
    Manhattan,
    Chebyshev,
}

impl DistanceMetric {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Euclidean => quote! { DistanceMetric::Euclidean },
            Self::EuclideanSquared => quote! { DistanceMetric::EuclideanSquared },
            Self::Manhattan => quote! { DistanceMetric::Manhattan },
            Self::Chebyshev => quote! { DistanceMetric::Chebyshev },
        }
    }
}

/// Coordinate axis.
#[derive(Deserialize, Hash, Copy, Clone)]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::X => quote! { Axis::X },
            Self::Y => quote! { Axis::Y },
            Self::Z => quote! { Axis::Z },
        }
    }
}

/// Gradient tiling behavior.
#[derive(Deserialize, Hash, Copy, Clone)]
enum Tiling {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

impl Tiling {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::ClampToEdge => quote! { Tiling::ClampToEdge },
            Self::Repeat => quote! { Tiling::Repeat },
            Self::MirroredRepeat => quote! { Tiling::MirroredRepeat },
        }
    }
}

/// Caching or interpolation wrapper applied around an inner density function.
#[derive(Copy, Clone, Deserialize, PartialEq, Eq, Hash)]
enum WrapperType {
    Interpolated { cell_size_xz: i32, cell_size_y: i32 },
    Cache,
}

impl WrapperType {
    #[cfg(feature = "codegen")]
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Interpolated {
                cell_size_xz,
                cell_size_y,
            } => quote! {
                WrapperType::Interpolated {
                    cell_size_xz: #cell_size_xz,
                    cell_size_y: #cell_size_y,
                }
            },
            Self::Cache => quote! { WrapperType::Cache },
        }
    }
}

#[derive(Deserialize, Hash, Clone)]
struct NoiseData {
    #[serde(rename(deserialize = "noise"))]
    noise_id: String,
    #[serde(rename(deserialize = "xzScale"))]
    xz_scale: HashableF64,
    #[serde(rename(deserialize = "yScale"))]
    y_scale: HashableF64,
}

#[derive(Deserialize, Hash, Clone)]
struct ShiftedNoiseData {
    #[serde(rename(deserialize = "xzScale"))]
    xz_scale: HashableF64,
    #[serde(rename(deserialize = "yScale"))]
    y_scale: HashableF64,
    #[serde(rename(deserialize = "noise"))]
    noise_id: String,
}

#[derive(Deserialize, Hash, Clone)]
struct InterpolatedNoiseSamplerData {
    #[serde(rename(deserialize = "xzScale"))]
    xz_scale: HashableF64,
    #[serde(rename(deserialize = "yScale"))]
    y_scale: HashableF64,
    #[serde(rename(deserialize = "xzFactor"))]
    xz_factor: HashableF64,
    #[serde(rename(deserialize = "yFactor"))]
    y_factor: HashableF64,
    #[serde(rename(deserialize = "smearScaleMultiplier"))]
    smear_scale_multiplier: HashableF64,
}

#[derive(Deserialize, Hash, Clone)]
struct ClampedYGradientData {
    #[serde(rename(deserialize = "fromY"))]
    from_y: i32,
    #[serde(rename(deserialize = "toY"))]
    to_y: i32,
    #[serde(rename(deserialize = "fromValue"))]
    from_value: HashableF32,
    #[serde(rename(deserialize = "toValue"))]
    to_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct GradientData {
    axis: Axis,
    tiling: Tiling,
    from_coordinate: i32,
    to_coordinate: i32,
    from_value: HashableF32,
    to_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct DistanceToPointData {
    point: [i32; 3],
    metric: DistanceMetric,
}

#[derive(Deserialize, Hash, Clone)]
struct RoundingData {
    operation: RoundingOperation,
}

#[derive(Deserialize, Hash, Clone)]
struct BinaryData {
    #[serde(rename(deserialize = "type"))]
    operation: BinaryOperation,
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct LinearData {
    #[serde(rename(deserialize = "specificType"))]
    operation: LinearOperation,
    argument: HashableF32,
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct FindTopSurfaceData {
    #[serde(rename(deserialize = "lowerBound"))]
    lower_bound: i32,
    #[serde(rename(deserialize = "cellHeight"))]
    cell_height: i32,
}

#[derive(Deserialize, Hash, Clone)]
struct UnaryData {
    #[serde(rename(deserialize = "type"))]
    operation: UnaryOperation,
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct ClampData {
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct RangeChoiceData {
    #[serde(rename(deserialize = "minInclusive"))]
    min_inclusive: HashableF32,
    #[serde(rename(deserialize = "maxExclusive"))]
    max_exclusive: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct SplineData {
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

/// Deserialized representation of any density function node in the noise router tree.
#[derive(Deserialize, Hash, Clone)]
#[serde(tag = "_class", content = "value")]
enum DensityFunctionRepr {
    Beardifier,
    BlendAlpha,
    BlendOffset,
    BlendDensity {
        input: Box<Self>,
    },
    FindTopSurface {
        density: Box<Self>,
        #[serde(rename(deserialize = "upperBound"))]
        upper_bound: Box<Self>,
        #[serde(flatten)]
        data: FindTopSurfaceData,
    },
    EndIslands,
    Noise {
        #[serde(flatten)]
        data: NoiseData,
    },
    ShiftA {
        #[serde(rename(deserialize = "offsetNoise"))]
        noise_id: String,
    },
    ShiftB {
        #[serde(rename(deserialize = "offsetNoise"))]
        noise_id: String,
    },
    ShiftedNoise {
        #[serde(rename(deserialize = "shiftX"))]
        shift_x: Box<Self>,
        #[serde(rename(deserialize = "shiftY"))]
        shift_y: Box<Self>,
        #[serde(rename(deserialize = "shiftZ"))]
        shift_z: Box<Self>,
        #[serde(flatten)]
        data: ShiftedNoiseData,
    },
    InterpolatedNoiseSampler {
        #[serde(flatten)]
        data: InterpolatedNoiseSamplerData,
    },
    #[serde(rename(deserialize = "IntervalSelect"))]
    IntervalSelect {
        input: Box<Self>,
        thresholds: Box<[HashableF32]>,
        functions: Box<[Self]>,
    },
    #[serde(rename(deserialize = "Wrapping"))]
    Wrapper {
        #[serde(rename(deserialize = "wrapped"))]
        input: Box<Self>,
        #[serde(rename(deserialize = "type"))]
        wrapper: WrapperType,
    },
    Constant {
        value: HashableF32,
    },
    #[serde(rename(deserialize = "YClampedGradient"))]
    ClampedYGradient {
        #[serde(flatten)]
        data: ClampedYGradientData,
    },
    Gradient {
        data: GradientData,
    },
    DistanceToPoint {
        data: DistanceToPointData,
    },
    Lerp {
        alpha: Box<Self>,
        first: Box<Self>,
        second: Box<Self>,
    },
    Rounding {
        input: Box<Self>,
        multiple: Box<Self>,
        data: RoundingData,
    },
    Slice {
        axis: Axis,
        coordinate: i32,
        input: Box<Self>,
    },
    #[serde(rename(deserialize = "BinaryOperation"))]
    Binary {
        argument1: Box<Self>,
        argument2: Box<Self>,
        #[serde(flatten)]
        data: BinaryData,
    },
    #[serde(rename(deserialize = "LinearOperation"))]
    Linear {
        input: Box<Self>,
        #[serde(flatten)]
        data: LinearData,
    },
    #[serde(rename(deserialize = "UnaryOperation"))]
    Unary {
        input: Box<Self>,
        #[serde(flatten)]
        data: UnaryData,
    },
    Clamp {
        input: Box<Self>,
        #[serde(flatten)]
        data: ClampData,
    },
    RangeChoice {
        input: Box<Self>,
        #[serde(rename(deserialize = "whenInRange"))]
        when_in_range: Box<Self>,
        #[serde(rename(deserialize = "whenOutOfRange"))]
        when_out_range: Box<Self>,
        #[serde(flatten)]
        data: RangeChoiceData,
    },
    Spline {
        spline: SplineRepr,
        #[serde(flatten)]
        data: SplineData,
    },
}

const AXIS_X: u8 = 1;
const AXIS_Y: u8 = 2;
const AXIS_Z: u8 = 4;
const AXES_XZ: u8 = AXIS_X | AXIS_Z;
const AXES_ALL: u8 = AXIS_X | AXIS_Y | AXIS_Z;

impl Axis {
    const fn as_axes(self) -> u8 {
        match self {
            Self::X => AXIS_X,
            Self::Y => AXIS_Y,
            Self::Z => AXIS_Z,
        }
    }
}

fn noise_domain_axes(xz_scale: f64, y_scale: f64) -> u8 {
    let mut axes = AXES_ALL;
    if y_scale == 0.0 {
        axes &= !AXIS_Y;
    }
    if xz_scale == 0.0 {
        axes &= !AXES_XZ;
    }
    axes
}

impl SplineRepr {
    fn for_each_function(&mut self, f: &mut dyn FnMut(&mut DensityFunctionRepr)) {
        if let Self::Standard {
            location_function,
            values,
            ..
        } = self
        {
            f(location_function);
            for value in values.iter_mut() {
                value.for_each_function(f);
            }
        }
    }

    fn domain_axes(&self) -> u8 {
        match self {
            Self::Fixed { .. } => 0,
            Self::Standard {
                location_function,
                values,
                ..
            } => values
                .iter()
                .fold(location_function.domain_axes(), |axes, value| {
                    axes | value.domain_axes()
                }),
        }
    }
}

impl DensityFunctionRepr {
    fn domain_axes(&self) -> u8 {
        match self {
            Self::Constant { .. } => 0,
            Self::BlendAlpha
            | Self::BlendOffset
            | Self::EndIslands
            | Self::ShiftA { .. }
            | Self::ShiftB { .. } => AXES_XZ,
            Self::Beardifier
            | Self::InterpolatedNoiseSampler { .. }
            | Self::DistanceToPoint { .. } => AXES_ALL,
            Self::ClampedYGradient { .. } => AXIS_Y,
            Self::Gradient { data } => data.axis.as_axes(),
            Self::Noise { data } => noise_domain_axes(data.xz_scale.0, data.y_scale.0),
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => {
                noise_domain_axes(data.xz_scale.0, data.y_scale.0)
                    | shift_x.domain_axes()
                    | shift_y.domain_axes()
                    | shift_z.domain_axes()
            }
            Self::BlendDensity { input }
            | Self::Wrapper { input, .. }
            | Self::Linear { input, .. }
            | Self::Unary { input, .. }
            | Self::Clamp { input, .. } => input.domain_axes(),
            Self::Slice { axis, input, .. } => input.domain_axes() & !axis.as_axes(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => (density.domain_axes() | upper_bound.domain_axes()) & !AXIS_Y,
            Self::IntervalSelect {
                input, functions, ..
            } => functions
                .iter()
                .fold(input.domain_axes(), |axes, f| axes | f.domain_axes()),
            Self::Lerp {
                alpha,
                first,
                second,
            } => alpha.domain_axes() | first.domain_axes() | second.domain_axes(),
            Self::Rounding {
                input, multiple, ..
            } => input.domain_axes() | multiple.domain_axes(),
            Self::Binary {
                argument1,
                argument2,
                ..
            } => argument1.domain_axes() | argument2.domain_axes(),
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                ..
            } => input.domain_axes() | when_in_range.domain_axes() | when_out_range.domain_axes(),
            Self::Spline { spline, .. } => spline.domain_axes(),
        }
    }

    fn for_each_child(&mut self, f: &mut dyn FnMut(&mut Self)) {
        match self {
            Self::Beardifier
            | Self::BlendAlpha
            | Self::BlendOffset
            | Self::EndIslands
            | Self::Noise { .. }
            | Self::ShiftA { .. }
            | Self::ShiftB { .. }
            | Self::InterpolatedNoiseSampler { .. }
            | Self::Constant { .. }
            | Self::ClampedYGradient { .. }
            | Self::Gradient { .. }
            | Self::DistanceToPoint { .. } => {}
            Self::BlendDensity { input }
            | Self::Wrapper { input, .. }
            | Self::Linear { input, .. }
            | Self::Unary { input, .. }
            | Self::Clamp { input, .. }
            | Self::Slice { input, .. } => f(input),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                f(density);
                f(upper_bound);
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                f(shift_x);
                f(shift_y);
                f(shift_z);
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                f(input);
                for function in functions.iter_mut() {
                    f(function);
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                f(alpha);
                f(first);
                f(second);
            }
            Self::Rounding {
                input, multiple, ..
            } => {
                f(input);
                f(multiple);
            }
            Self::Binary {
                argument1,
                argument2,
                ..
            } => {
                f(argument1);
                f(argument2);
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                ..
            } => {
                f(input);
                f(when_in_range);
                f(when_out_range);
            }
            Self::Spline { spline, .. } => spline.for_each_function(f),
        }
    }

    fn existing_removed_axes(&self) -> u8 {
        let mut axes = 0;
        let mut function = self;
        while let Self::Slice { axis, input, .. } = function {
            axes |= axis.as_axes();
            function = input;
        }
        axes
    }

    fn remove_axes(&mut self, axes: u8) {
        let filtered = axes & !self.existing_removed_axes();
        for (bit, axis) in [(AXIS_X, Axis::X), (AXIS_Z, Axis::Z), (AXIS_Y, Axis::Y)] {
            if filtered & bit != 0 {
                let input = std::mem::replace(
                    self,
                    Self::Constant {
                        value: HashableF32(0.0),
                    },
                );
                *self = Self::Slice {
                    axis,
                    coordinate: 0,
                    input: Box::new(input),
                };
            }
        }
    }

    fn slice_uniform_axes(&mut self, parent_axes: u8) {
        if matches!(
            self,
            Self::Constant { .. } | Self::Gradient { .. } | Self::ClampedYGradient { .. }
        ) {
            return;
        }
        let axes = self.domain_axes();
        if let Self::FindTopSurface {
            density,
            upper_bound,
            ..
        } = self
        {
            density.slice_uniform_axes(axes | AXIS_Y);
            upper_bound.slice_uniform_axes(axes);
        } else {
            self.for_each_child(&mut |child| child.slice_uniform_axes(axes));
        }
        if parent_axes != axes {
            self.remove_axes(parent_axes & !axes);
        }
    }

    #[expect(clippy::too_many_lines, reason = "one branch per density node")]
    fn optimize(&mut self) {
        match self {
            Self::BlendDensity { input }
            | Self::Slice { input, .. }
            | Self::Wrapper { input, .. } => input.optimize(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                density.optimize();
                upper_bound.optimize();
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                shift_x.optimize();
                shift_y.optimize();
                shift_z.optimize();
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                input.optimize();
                for f in functions.iter_mut() {
                    f.optimize();
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                alpha.optimize();
                first.optimize();
                second.optimize();
            }
            Self::Rounding {
                input, multiple, ..
            } => {
                input.optimize();
                multiple.optimize();
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                ..
            } => {
                input.optimize();
                when_in_range.optimize();
                when_out_range.optimize();
            }
            Self::Linear { input, data } => {
                input.optimize();
                if let Self::Constant { value } = &**input {
                    let val = match data.operation {
                        LinearOperation::Add => value.0 + data.argument.0,
                        LinearOperation::Mul => value.0 * data.argument.0,
                    };
                    *self = Self::Constant {
                        value: HashableF32(val),
                    };
                }
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => {
                argument1.optimize();
                argument2.optimize();
                if let (Self::Constant { value: v1 }, Self::Constant { value: v2 }) =
                    (&**argument1, &**argument2)
                {
                    let res = match data.operation {
                        BinaryOperation::Add => v1.0 + v2.0,
                        BinaryOperation::Mul => v1.0 * v2.0,
                        BinaryOperation::Min => v1.0.min(v2.0),
                        BinaryOperation::Max => v1.0.max(v2.0),
                        BinaryOperation::Sub => v1.0 - v2.0,
                        BinaryOperation::Div => {
                            if v2.0 == 0.0 {
                                0.0
                            } else {
                                v1.0 / v2.0
                            }
                        }
                        BinaryOperation::Pow => v1.0.powf(v2.0),
                    };
                    *self = Self::Constant {
                        value: HashableF32(res),
                    };
                }
            }
            Self::Unary { input, data } => {
                input.optimize();
                if let Self::Constant { value } = &**input {
                    let val = match data.operation {
                        UnaryOperation::Abs => value.0.abs(),
                        UnaryOperation::Square => value.0 * value.0,
                        UnaryOperation::Cube => value.0 * value.0 * value.0,
                        UnaryOperation::HalfNegative => {
                            if value.0 > 0.0 {
                                value.0
                            } else {
                                value.0 * 0.5
                            }
                        }
                        UnaryOperation::QuarterNegative => {
                            if value.0 > 0.0 {
                                value.0
                            } else {
                                value.0 * 0.25
                            }
                        }
                        UnaryOperation::Squeeze => {
                            let c = value.0.clamp(-1.0, 1.0);
                            c / 2.0 - c * c * c / 24.0
                        }
                        UnaryOperation::Invert => {
                            if value.0 == 0.0 {
                                f32::INFINITY
                            } else {
                                1.0 / value.0
                            }
                        }
                        UnaryOperation::Negate => -value.0,
                        UnaryOperation::Sqrt => value.0.sqrt(),
                        UnaryOperation::Log => value.0.ln(),
                        UnaryOperation::Sign => {
                            if value.0 > 0.0 {
                                1.0
                            } else if value.0 < 0.0 {
                                -1.0
                            } else {
                                0.0
                            }
                        }
                    };
                    *self = Self::Constant {
                        value: HashableF32(val),
                    };
                }
            }
            Self::Clamp { input, data } => {
                input.optimize();
                if let Self::Constant { value } = &**input {
                    *self = Self::Constant {
                        value: HashableF32(value.0.clamp(data.min_value.0, data.max_value.0)),
                    };
                }
            }
            _ => {}
        }
    }

    #[cfg(feature = "codegen")]
    fn get_index_for_component_readonly(&self, hash_to_index_map: &BTreeMap<u64, usize>) -> usize {
        *hash_to_index_map.get(&self.unique_id()).unwrap_or(&0)
    }

    #[cfg(feature = "codegen")]
    #[expect(clippy::too_many_lines, reason = "one emitter per density node")]
    fn emit_compiled_eval_fn(
        &self,
        index: usize,
        fn_prefix: &str,
        hash_to_index_map: &BTreeMap<u64, usize>,
    ) -> TokenStream {
        let fn_name = syn::Ident::new(&format!("{fn_prefix}_{index}"), Span::call_site());
        match self {
            Self::Constant { value } => {
                let val = value.0;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let _ = (pos, ctx);
                        #val
                    }
                }
            }
            Self::ClampedYGradient { data } => {
                let from_y = data.from_y as f32;
                let to_y = data.to_y as f32;
                let from_val = data.from_value.0;
                let to_val = data.to_value.0;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let _ = ctx;
                        let y = pos.y as f32;
                        let clamped = y.clamp(#from_y, #to_y);
                        let delta = (clamped - #from_y) / (#to_y - #from_y);
                        #from_val + delta * (#to_val - #from_val)
                    }
                }
            }
            Self::Gradient { data } => {
                let from_coord = data.from_coordinate;
                let to_coord = data.to_coordinate;
                let from_val = data.from_value.0;
                let to_val = data.to_value.0;
                let coord = match data.axis {
                    Axis::X => quote! { pos.x },
                    Axis::Y => quote! { pos.y },
                    Axis::Z => quote! { pos.z },
                };
                let range = to_coord - from_coord;
                let factor = (to_val - from_val) / (range as f32);
                let body = match data.tiling {
                    Tiling::ClampToEdge => {
                        let min_c = from_coord.min(to_coord);
                        let max_c = from_coord.max(to_coord);
                        quote! {
                            let rel = coord.clamp(#min_c, #max_c) - #from_coord;
                            #from_val + rel as f32 * #factor
                        }
                    }
                    Tiling::Repeat => quote! {
                        let rel = coord - #from_coord;
                        #from_val + rel.rem_euclid(#range) as f32 * #factor
                    },
                    Tiling::MirroredRepeat => quote! {
                        let rel = coord - #from_coord;
                        let tile = rel.div_euclid(#range);
                        let local = rel - tile * #range;
                        if (tile & 1) == 0 {
                            #from_val + local as f32 * #factor
                        } else {
                            #from_val + (#range - local) as f32 * #factor
                        }
                    },
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let _ = ctx;
                        let coord = #coord;
                        #body
                    }
                }
            }
            Self::DistanceToPoint { data } => {
                let px = data.point[0];
                let py = data.point[1];
                let pz = data.point[2];
                let body = match data.metric {
                    DistanceMetric::Euclidean => quote! { (dx * dx + dy * dy + dz * dz).sqrt() },
                    DistanceMetric::EuclideanSquared => quote! { dx * dx + dy * dy + dz * dz },
                    DistanceMetric::Manhattan => quote! { dx.abs() + dy.abs() + dz.abs() },
                    DistanceMetric::Chebyshev => quote! { dx.abs().max(dy.abs()).max(dz.abs()) },
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let _ = ctx;
                        let dx = (pos.x - #px) as f32;
                        let dy = (pos.y - #py) as f32;
                        let dz = (pos.z - #pz) as f32;
                        #body
                    }
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let a_idx = alpha.get_index_for_component_readonly(hash_to_index_map);
                let f_idx = first.get_index_for_component_readonly(hash_to_index_map);
                let s_idx = second.get_index_for_component_readonly(hash_to_index_map);
                let a_fn = syn::Ident::new(&format!("{fn_prefix}_{a_idx}"), Span::call_site());
                let f_fn = syn::Ident::new(&format!("{fn_prefix}_{f_idx}"), Span::call_site());
                let s_fn = syn::Ident::new(&format!("{fn_prefix}_{s_idx}"), Span::call_site());
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let a = #a_fn(pos, ctx);
                        let f = #f_fn(pos, ctx);
                        let s = #s_fn(pos, ctx);
                        f + a * (s - f)
                    }
                }
            }
            Self::Rounding {
                input,
                multiple,
                data,
            } => {
                let in_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let mul_idx = multiple.get_index_for_component_readonly(hash_to_index_map);
                let in_fn = syn::Ident::new(&format!("{fn_prefix}_{in_idx}"), Span::call_site());
                let mul_fn = syn::Ident::new(&format!("{fn_prefix}_{mul_idx}"), Span::call_site());
                let body = match data.operation {
                    RoundingOperation::Floor => {
                        quote! { if m == 0.0 { v } else { (v / m).floor() * m } }
                    }
                    RoundingOperation::Round => {
                        quote! { if m == 0.0 { v } else { (v / m + 0.5).floor() * m } }
                    }
                    RoundingOperation::Ceil => {
                        quote! { if m == 0.0 { v } else { (v / m).ceil() * m } }
                    }
                    RoundingOperation::Truncate => {
                        quote! { if m == 0.0 { v } else { let d = v / m; if d > 0.0 { d.floor() * m } else { d.ceil() * m } } }
                    }
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let v = #in_fn(pos, ctx);
                        let m = #mul_fn(pos, ctx);
                        #body
                    }
                }
            }
            Self::Slice {
                axis,
                coordinate,
                input,
            } => {
                let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let child_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child_idx}"), Span::call_site());
                let slice_pos = match axis {
                    Axis::X => {
                        quote! { pumpkin_util::math::vector3::Vector3::new(#coordinate, pos.y, pos.z) }
                    }
                    Axis::Y => {
                        quote! { pumpkin_util::math::vector3::Vector3::new(pos.x, #coordinate, pos.z) }
                    }
                    Axis::Z => {
                        quote! { pumpkin_util::math::vector3::Vector3::new(pos.x, pos.y, #coordinate) }
                    }
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let slice_pos = #slice_pos;
                        #child_fn(&slice_pos, ctx)
                    }
                }
            }
            Self::Linear { input, data } => {
                let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let child_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child_idx}"), Span::call_site());
                let arg = data.argument.0;
                let body = match data.operation {
                    LinearOperation::Add => quote! { #child_fn(pos, ctx) + #arg },
                    LinearOperation::Mul => quote! { #child_fn(pos, ctx) * #arg },
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        #body
                    }
                }
            }
            Self::Unary { input, data } => {
                let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let child_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child_idx}"), Span::call_site());
                let body = match data.operation {
                    UnaryOperation::Abs => quote! { #child_fn(pos, ctx).abs() },
                    UnaryOperation::Square => quote! { let v = #child_fn(pos, ctx); v * v },
                    UnaryOperation::Cube => quote! { let v = #child_fn(pos, ctx); v * v * v },
                    UnaryOperation::HalfNegative => {
                        quote! { let v = #child_fn(pos, ctx); if v > 0.0 { v } else { v * 0.5 } }
                    }
                    UnaryOperation::QuarterNegative => {
                        quote! { let v = #child_fn(pos, ctx); if v > 0.0 { v } else { v * 0.25 } }
                    }
                    UnaryOperation::Squeeze => {
                        quote! { let c = #child_fn(pos, ctx).clamp(-1.0, 1.0); c / 2.0 - c * c * c / 24.0 }
                    }
                    UnaryOperation::Invert => {
                        quote! { let v = #child_fn(pos, ctx); if v == 0.0 { f32::INFINITY } else { 1.0 / v } }
                    }
                    UnaryOperation::Negate => quote! { -#child_fn(pos, ctx) },
                    UnaryOperation::Sqrt => quote! { #child_fn(pos, ctx).sqrt() },
                    UnaryOperation::Log => quote! { #child_fn(pos, ctx).ln() },
                    UnaryOperation::Sign => {
                        quote! { let v = #child_fn(pos, ctx); if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 } }
                    }
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        #body
                    }
                }
            }
            Self::Clamp { input, data } => {
                let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let child_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child_idx}"), Span::call_site());
                let min_v = data.min_value.0;
                let max_v = data.max_value.0;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        #child_fn(pos, ctx).clamp(#min_v, #max_v)
                    }
                }
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => {
                let child1_idx = argument1.get_index_for_component_readonly(hash_to_index_map);
                let child2_idx = argument2.get_index_for_component_readonly(hash_to_index_map);
                let child1_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child1_idx}"), Span::call_site());
                let child2_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child2_idx}"), Span::call_site());
                let body = match data.operation {
                    BinaryOperation::Add => quote! { #child1_fn(pos, ctx) + #child2_fn(pos, ctx) },
                    BinaryOperation::Mul => quote! { #child1_fn(pos, ctx) * #child2_fn(pos, ctx) },
                    BinaryOperation::Min => {
                        quote! { #child1_fn(pos, ctx).min(#child2_fn(pos, ctx)) }
                    }
                    BinaryOperation::Max => {
                        quote! { #child1_fn(pos, ctx).max(#child2_fn(pos, ctx)) }
                    }
                    BinaryOperation::Sub => quote! { #child1_fn(pos, ctx) - #child2_fn(pos, ctx) },
                    BinaryOperation::Div => {
                        quote! { let b = #child2_fn(pos, ctx); if b == 0.0 { 0.0 } else { #child1_fn(pos, ctx) / b } }
                    }
                    BinaryOperation::Pow => {
                        quote! { #child1_fn(pos, ctx).powf(#child2_fn(pos, ctx)) }
                    }
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        #body
                    }
                }
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                data,
            } => {
                let input_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let when_in_idx = when_in_range.get_index_for_component_readonly(hash_to_index_map);
                let when_out_idx =
                    when_out_range.get_index_for_component_readonly(hash_to_index_map);
                let input_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{input_idx}"), Span::call_site());
                let when_in_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{when_in_idx}"), Span::call_site());
                let when_out_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{when_out_idx}"), Span::call_site());
                let min_inc = data.min_inclusive.0;
                let max_exc = data.max_exclusive.0;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let val = #input_fn(pos, ctx);
                        if val >= #min_inc && val < #max_exc {
                            #when_in_fn(pos, ctx)
                        } else {
                            #when_out_fn(pos, ctx)
                        }
                    }
                }
            }
            Self::Noise { data } => {
                let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());
                let xz_scale = data.xz_scale.0;
                let y_scale = data.y_scale.0;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_noise(DoublePerlinNoiseParameters::#noise_id, f64::from(pos.x) * #xz_scale, f64::from(pos.y) * #y_scale, f64::from(pos.z) * #xz_scale)
                    }
                }
            }
            Self::ShiftA { noise_id } => {
                let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_shift_a(DoublePerlinNoiseParameters::#noise_id, pos)
                    }
                }
            }
            Self::ShiftB { noise_id } => {
                let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_shift_b(DoublePerlinNoiseParameters::#noise_id, pos)
                    }
                }
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => {
                let sx_idx = shift_x.get_index_for_component_readonly(hash_to_index_map);
                let sy_idx = shift_y.get_index_for_component_readonly(hash_to_index_map);
                let sz_idx = shift_z.get_index_for_component_readonly(hash_to_index_map);
                let sx_fn = syn::Ident::new(&format!("{fn_prefix}_{sx_idx}"), Span::call_site());
                let sy_fn = syn::Ident::new(&format!("{fn_prefix}_{sy_idx}"), Span::call_site());
                let sz_fn = syn::Ident::new(&format!("{fn_prefix}_{sz_idx}"), Span::call_site());
                let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());
                let xz_scale = data.xz_scale.0;
                let y_scale = data.y_scale.0;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let sx = #sx_fn(pos, ctx);
                        let sy = #sy_fn(pos, ctx);
                        let sz = #sz_fn(pos, ctx);
                        ctx.sample_shifted_noise(DoublePerlinNoiseParameters::#noise_id, sx, sy, sz, #xz_scale, #y_scale)
                    }
                }
            }
            Self::BlendAlpha => {
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_blend_alpha(pos)
                    }
                }
            }
            Self::BlendOffset => {
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_blend_offset(pos)
                    }
                }
            }
            Self::BlendDensity { input } => {
                let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let child_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child_idx}"), Span::call_site());
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let val = #child_fn(pos, ctx);
                        ctx.sample_blend_density(val, pos)
                    }
                }
            }
            Self::Beardifier => {
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_beardifier(pos)
                    }
                }
            }
            Self::EndIslands => {
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_end_islands(pos)
                    }
                }
            }
            Self::Wrapper { input, wrapper } => {
                let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let child_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{child_idx}"), Span::call_site());
                let wrapper_repr = wrapper.into_token_stream();
                let comp_idx = index;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_wrapper(#comp_idx, #wrapper_repr, pos, &#child_fn)
                    }
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let input_idx = input.get_index_for_component_readonly(hash_to_index_map);
                let input_fn =
                    syn::Ident::new(&format!("{fn_prefix}_{input_idx}"), Span::call_site());
                let func_fns = functions
                    .iter()
                    .map(|f| {
                        let idx = f.get_index_for_component_readonly(hash_to_index_map);
                        syn::Ident::new(&format!("{fn_prefix}_{idx}"), Span::call_site())
                    })
                    .collect::<Vec<_>>();
                let threshold_values = thresholds.iter().map(|t| t.0).collect::<Vec<_>>();
                let th_indices = (0..threshold_values.len()).collect::<Vec<_>>();
                #[expect(
                    clippy::unwrap_used,
                    reason = "interval validation requires at least one function"
                )]
                let last_func_fn = func_fns.last().unwrap();
                let initial_func_fns = if func_fns.len() > 1 {
                    &func_fns[..func_fns.len() - 1]
                } else {
                    &[]
                };
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        let input_val = #input_fn(pos, ctx);
                        let thresholds = &[#(#threshold_values),*];
                        let mut selected = thresholds.len();
                        for (i, &t) in thresholds.iter().enumerate() {
                            if input_val < t {
                                selected = i;
                                break;
                            }
                        }
                        match selected {
                            #( #th_indices => #initial_func_fns(pos, ctx), )*
                            _ => #last_func_fn(pos, ctx),
                        }
                    }
                }
            }
            Self::InterpolatedNoiseSampler { .. } => {
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_interpolated_noise(pos)
                    }
                }
            }
            Self::Spline { spline, .. } => match spline {
                SplineRepr::Standard {
                    location_function, ..
                } => {
                    let loc_idx =
                        location_function.get_index_for_component_readonly(hash_to_index_map);
                    let loc_fn =
                        syn::Ident::new(&format!("{fn_prefix}_{loc_idx}"), Span::call_site());
                    quote! {
                        #[inline(always)]
                        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                            let location_val = #loc_fn(pos, ctx);
                            ctx.sample_spline(#index, location_val, pos)
                        }
                    }
                }
                SplineRepr::Fixed { value } => {
                    let val = value.0;
                    quote! {
                        #[inline(always)]
                        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                            let _ = (pos, ctx);
                            #val
                        }
                    }
                }
            },
            Self::FindTopSurface {
                density,
                upper_bound,
                data,
            } => {
                let d_idx = density.get_index_for_component_readonly(hash_to_index_map);
                let u_idx = upper_bound.get_index_for_component_readonly(hash_to_index_map);
                let d_fn = syn::Ident::new(&format!("{fn_prefix}_{d_idx}"), Span::call_site());
                let u_fn = syn::Ident::new(&format!("{fn_prefix}_{u_idx}"), Span::call_site());
                let lower = data.lower_bound;
                let cell_h = data.cell_height;
                quote! {
                    #[inline(always)]
                    pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                        ctx.sample_find_top_surface(&#d_fn, &#u_fn, #lower, #cell_h, pos)
                    }
                }
            }
        }
    }

    fn unique_id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    #[cfg(feature = "codegen")]
    fn get_index_for_component(
        &self,
        stack: &mut Vec<TokenStream>,
        nodes: &mut Vec<Self>,
        hash_to_index_map: &mut BTreeMap<u64, usize>,
    ) -> usize {
        let id = self.unique_id();
        if let Some(&index) = hash_to_index_map.get(&id) {
            return index;
        }
        let repr = self.get_token_stream(stack, nodes, hash_to_index_map);
        stack.push(repr);
        nodes.push(self.clone());
        let index = stack.len() - 1;
        hash_to_index_map.insert(id, index);
        index
    }

    #[cfg(feature = "codegen")]
    #[expect(clippy::too_many_lines, reason = "one emitter per density node")]
    fn get_token_stream(
        &self,
        stack: &mut Vec<TokenStream>,
        nodes: &mut Vec<Self>,
        hash_to_index_map: &mut BTreeMap<u64, usize>,
    ) -> TokenStream {
        match self {
            Self::Spline { spline, data } => {
                let _ = data;
                let spline_repr = spline.get_token_stream(stack, nodes, hash_to_index_map);

                quote! {
                    BaseNoiseFunctionComponent::Spline {
                        spline: &#spline_repr,
                    }
                }
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                data,
            } => {
                let density_index =
                    density.get_index_for_component(stack, nodes, hash_to_index_map);
                let upper_bound_index =
                    upper_bound.get_index_for_component(stack, nodes, hash_to_index_map);
                let lower_bound = data.lower_bound;
                let cell_height = data.cell_height;

                quote! {
                    BaseNoiseFunctionComponent::FindTopSurface {
                        density_index: #density_index,
                        upper_bound_index: #upper_bound_index,
                        data: &FindTopSurfaceData {
                            lower_bound: #lower_bound,
                            cell_height: #cell_height,
                        },
                    }
                }
            }
            Self::EndIslands => quote! {
                BaseNoiseFunctionComponent::EndIslands
            },
            Self::Noise { data } => {
                let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());
                let xz_scale = &data.xz_scale;
                let y_scale = &data.y_scale;

                quote! {
                    BaseNoiseFunctionComponent::Noise {
                        data: &NoiseData {
                            noise_id: DoublePerlinNoiseParameters::#noise_id,
                            xz_scale: #xz_scale,
                            y_scale: #y_scale,
                        }
                    }
                }
            }
            Self::ShiftA { noise_id } => {
                let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());

                quote! {
                    BaseNoiseFunctionComponent::ShiftA {
                        noise_id: DoublePerlinNoiseParameters::#noise_id,
                    }
                }
            }
            Self::ShiftB { noise_id } => {
                let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());

                quote! {
                    BaseNoiseFunctionComponent::ShiftB {
                        noise_id: DoublePerlinNoiseParameters::#noise_id,
                    }
                }
            }
            Self::BlendDensity { input } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                quote! {
                    BaseNoiseFunctionComponent::BlendDensity {
                        input_index: #input_index,
                    }
                }
            }
            Self::BlendAlpha => {
                quote! {
                    BaseNoiseFunctionComponent::BlendAlpha
                }
            }
            Self::BlendOffset => {
                quote! {
                    BaseNoiseFunctionComponent::BlendOffset
                }
            }
            Self::Beardifier => {
                quote! {
                    BaseNoiseFunctionComponent::Beardifier
                }
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => {
                let shift_x_index =
                    shift_x.get_index_for_component(stack, nodes, hash_to_index_map);
                let shift_y_index =
                    shift_y.get_index_for_component(stack, nodes, hash_to_index_map);
                let shift_z_index =
                    shift_z.get_index_for_component(stack, nodes, hash_to_index_map);

                let xz_scale = &data.xz_scale;
                let y_scale = &data.y_scale;
                let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());

                quote! {
                    BaseNoiseFunctionComponent::ShiftedNoise {
                        shift_x_index: #shift_x_index,
                        shift_y_index: #shift_y_index,
                        shift_z_index: #shift_z_index,
                        data: &ShiftedNoiseData {
                            xz_scale: #xz_scale,
                            y_scale: #y_scale,
                            noise_id: DoublePerlinNoiseParameters::#noise_id,
                        },
                    }
                }
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                data,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let when_in_index =
                    when_in_range.get_index_for_component(stack, nodes, hash_to_index_map);
                let when_out_index =
                    when_out_range.get_index_for_component(stack, nodes, hash_to_index_map);

                let min_inclusive = &data.min_inclusive;
                let max_exclusive = &data.max_exclusive;

                quote! {
                    BaseNoiseFunctionComponent::RangeChoice {
                        input_index: #input_index,
                        when_in_range_index: #when_in_index,
                        when_out_range_index: #when_out_index,
                        data: &RangeChoiceData {
                            min_inclusive: #min_inclusive,
                            max_exclusive: #max_exclusive,
                        },
                    }
                }
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => {
                let argument1_index =
                    argument1.get_index_for_component(stack, nodes, hash_to_index_map);
                let argument2_index =
                    argument2.get_index_for_component(stack, nodes, hash_to_index_map);

                let action = data.operation.get_token_stream();
                quote! {
                    BaseNoiseFunctionComponent::Binary {
                        argument1_index: #argument1_index,
                        argument2_index: #argument2_index,
                        data: &BinaryData {
                            operation: #action,
                        },
                    }
                }
            }
            Self::ClampedYGradient { data } => {
                let from_y = data.from_y as f32;
                let to_y = data.to_y as f32;
                let from_value = &data.from_value;
                let to_value = &data.to_value;

                quote! {
                    BaseNoiseFunctionComponent::ClampedYGradient {
                        data: &ClampedYGradientData {
                            from_y: #from_y,
                            to_y: #to_y,
                            from_value: #from_value,
                            to_value: #to_value,
                        }
                    }
                }
            }
            Self::Gradient { data } => {
                let axis = data.axis.into_token_stream();
                let tiling = data.tiling.into_token_stream();
                let from_coordinate = data.from_coordinate;
                let to_coordinate = data.to_coordinate;
                let from_value = &data.from_value;
                let to_value = &data.to_value;

                quote! {
                    BaseNoiseFunctionComponent::Gradient {
                        data: &GradientData {
                            axis: #axis,
                            tiling: #tiling,
                            from_coordinate: #from_coordinate,
                            to_coordinate: #to_coordinate,
                            from_value: #from_value,
                            to_value: #to_value,
                        }
                    }
                }
            }
            Self::DistanceToPoint { data } => {
                let px = data.point[0];
                let py = data.point[1];
                let pz = data.point[2];
                let metric = data.metric.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::DistanceToPoint {
                        data: &DistanceToPointData {
                            point: [#px, #py, #pz],
                            metric: #metric,
                        },
                    }
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha_index = alpha.get_index_for_component(stack, nodes, hash_to_index_map);
                let first_index = first.get_index_for_component(stack, nodes, hash_to_index_map);
                let second_index = second.get_index_for_component(stack, nodes, hash_to_index_map);

                quote! {
                    BaseNoiseFunctionComponent::Lerp {
                        alpha_index: #alpha_index,
                        first_index: #first_index,
                        second_index: #second_index,
                    }
                }
            }
            Self::Rounding {
                input,
                multiple,
                data,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let multiple_index =
                    multiple.get_index_for_component(stack, nodes, hash_to_index_map);
                let action = data.operation.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Rounding {
                        input_index: #input_index,
                        multiple_index: #multiple_index,
                        data: &RoundingData {
                            operation: #action,
                        },
                    }
                }
            }
            Self::Slice {
                axis,
                coordinate,
                input,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let axis_ts = axis.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Slice {
                        input_index: #input_index,
                        axis: #axis_ts,
                        coordinate: #coordinate,
                    }
                }
            }
            Self::Constant { value } => {
                quote! {
                    BaseNoiseFunctionComponent::Constant {
                        value: #value
                    }
                }
            }
            Self::Wrapper { input, wrapper } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let wrapper_repr = wrapper.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Wrapper {
                        input_index: #input_index,
                        wrapper: #wrapper_repr,
                    }
                }
            }
            Self::Linear { input, data } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                let action = data.operation.into_token_stream();
                let argument = &data.argument;
                quote! {
                    BaseNoiseFunctionComponent::Linear {
                        input_index: #input_index,
                        data: &LinearData {
                            operation: #action,
                            argument: #argument,
                        },
                    }
                }
            }
            Self::Clamp { input, data } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                let min_value = &data.min_value;
                let max_value = &data.max_value;

                quote! {
                    BaseNoiseFunctionComponent::Clamp {
                        input_index: #input_index,
                        data: &ClampData {
                            min_value: #min_value,
                            max_value: #max_value,
                        },
                    }
                }
            }
            Self::Unary { input, data } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                let action = data.operation.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Unary {
                        input_index: #input_index,
                        data: &UnaryData {
                            operation: #action,
                        },
                    }
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let functions_indices = functions
                    .iter()
                    .map(|f| f.get_index_for_component(stack, nodes, hash_to_index_map))
                    .collect::<Vec<_>>();
                let thresholds = thresholds.iter().map(|t| t.0).collect::<Vec<_>>();

                quote! {
                    BaseNoiseFunctionComponent::IntervalSelect {
                        input_index: #input_index,
                        thresholds: &[#(#thresholds),*],
                        functions_indices: &[#(#functions_indices),*],
                    }
                }
            }
            Self::InterpolatedNoiseSampler { data } => {
                let xz_scale = &data.xz_scale;
                let y_scale = &data.y_scale;
                let xz_factor = &data.xz_factor;
                let y_factor = &data.y_factor;
                let smear_scale_multiplier = &data.smear_scale_multiplier;

                quote! {
                    BaseNoiseFunctionComponent::InterpolatedNoiseSampler {
                        data: &InterpolatedNoiseSamplerData {
                            xz_scale: #xz_scale,
                            y_scale: #y_scale,
                            xz_factor: #xz_factor,
                            y_factor: #y_factor,
                            smear_scale_multiplier: #smear_scale_multiplier,
                        }
                    }
                }
            }
        }
    }
}

/// Deserialized noise router for a single dimension, containing all density function roots.
#[derive(Deserialize)]
pub struct NoiseRouterRepr {
    #[serde(rename(deserialize = "barrierNoise"))]
    barrier_noise: DensityFunctionRepr,
    #[serde(rename(deserialize = "fluidLevelFloodednessNoise"))]
    fluid_level_floodedness_noise: DensityFunctionRepr,
    #[serde(rename(deserialize = "fluidLevelSpreadNoise"))]
    fluid_level_spread_noise: DensityFunctionRepr,
    #[serde(rename(deserialize = "lavaNoise"))]
    lava_noise: DensityFunctionRepr,
    temperature: DensityFunctionRepr,
    vegetation: DensityFunctionRepr,
    continents: DensityFunctionRepr,
    erosion: DensityFunctionRepr,
    depth: DensityFunctionRepr,
    ridges: DensityFunctionRepr,
    #[serde(rename(deserialize = "preliminarySurfaceLevel"))]
    preliminary_surface_level: DensityFunctionRepr,
    #[serde(rename(deserialize = "finalDensity"))]
    final_density: DensityFunctionRepr,
    #[serde(rename(deserialize = "veinToggle"))]
    vein_toggle: DensityFunctionRepr,
    #[serde(rename(deserialize = "veinRidged"))]
    vein_ridged: DensityFunctionRepr,
    #[serde(rename(deserialize = "veinGap"))]
    vein_gap: DensityFunctionRepr,
}

impl NoiseRouterRepr {
    fn slice_uniform_axes(&mut self) {
        self.barrier_noise.slice_uniform_axes(AXES_ALL);
        self.fluid_level_floodedness_noise
            .slice_uniform_axes(AXES_ALL);
        self.fluid_level_spread_noise.slice_uniform_axes(AXES_ALL);
        self.lava_noise.slice_uniform_axes(AXES_ALL);
        self.temperature.slice_uniform_axes(AXES_ALL);
        self.vegetation.slice_uniform_axes(AXES_ALL);
        self.continents.slice_uniform_axes(AXES_ALL);
        self.erosion.slice_uniform_axes(AXES_ALL);
        self.depth.slice_uniform_axes(AXES_ALL);
        self.ridges.slice_uniform_axes(AXES_ALL);
        self.preliminary_surface_level.slice_uniform_axes(AXES_ALL);
        self.final_density.slice_uniform_axes(AXES_ALL);
        self.vein_toggle.slice_uniform_axes(AXES_ALL);
        self.vein_ridged.slice_uniform_axes(AXES_ALL);
        self.vein_gap.slice_uniform_axes(AXES_ALL);
    }

    fn optimize(&mut self) {
        self.barrier_noise.optimize();
        self.fluid_level_floodedness_noise.optimize();
        self.fluid_level_spread_noise.optimize();
        self.lava_noise.optimize();
        self.temperature.optimize();
        self.vegetation.optimize();
        self.continents.optimize();
        self.erosion.optimize();
        self.depth.optimize();
        self.ridges.optimize();
        self.preliminary_surface_level.optimize();
        self.final_density.optimize();
        self.vein_toggle.optimize();
        self.vein_ridged.optimize();
        self.vein_gap.optimize();
    }

    #[cfg(feature = "codegen")]
    #[must_use]
    pub fn into_token_stream_compiled(mut self, dim_name: &str) -> (TokenStream, TokenStream) {
        self.optimize();
        self.slice_uniform_axes();

        let mut noise = (Vec::new(), Vec::new(), BTreeMap::new());
        let mut surface = (Vec::new(), Vec::new(), BTreeMap::new());
        let mut multinoise = (Vec::new(), Vec::new(), BTreeMap::new());
        let mut vein = (Vec::new(), Vec::new(), BTreeMap::new());
        let Ok(RouterRoots {
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
        }) = self.visit_roots(
            [&mut noise, &mut surface, &mut multinoise, &mut vein],
            |node, (stack, nodes, indices)| {
                Ok::<_, std::convert::Infallible>(
                    node.get_index_for_component(stack, nodes, indices),
                )
            },
        );
        let (noise_component_stack, noise_nodes, noise_lookup_map) = noise;
        let (surface_component_stack, _, _) = surface;
        let (multinoise_component_stack, _, _) = multinoise;
        let (vein_component_stack, _, _) = vein;

        let mut compiled_fns = Vec::new();
        let fn_prefix = format!("eval_{dim_name}");
        for (i, node) in noise_nodes.iter().enumerate() {
            compiled_fns.push(node.emit_compiled_eval_fn(i, &fn_prefix, &noise_lookup_map));
        }

        let compiled_mod_ident =
            syn::Ident::new(&format!("{dim_name}_compiled"), Span::call_site());
        let compiled_mod_ts = quote! {
            pub mod #compiled_mod_ident {
                use super::*;
                #(#compiled_fns)*
            }
        };

        let base_routers_ts = quote! {
            BaseNoiseRouters {
                noise: BaseNoiseRouter {
                    full_component_stack: &[#(#noise_component_stack),*],
                    barrier_noise: #barrier_noise,
                    fluid_level_floodedness_noise: #fluid_level_floodedness_noise,
                    fluid_level_spread_noise: #fluid_level_spread_noise,
                    lava_noise: #lava_noise,
                    erosion: #noise_erosion,
                    depth: #noise_depth,
                    final_density: #final_density,
                    vein_toggle: #vein_toggle,
                    vein_ridged: #vein_ridged,
                    vein_gap: #vein_gap,
                },
                surface_estimator: BaseSurfaceEstimator {
                    full_component_stack: &[#(#surface_component_stack),*],
                },
                multi_noise: BaseMultiNoiseRouter {
                    full_component_stack: &[#(#multinoise_component_stack),*],
                    temperature: #temperature,
                    vegetation: #vegetation,
                    continents: #continents,
                    erosion: #multi_erosion,
                    depth: #multi_depth,
                    ridges: #ridges,
                },
                vein: BaseVeinRouter {
                    full_component_stack: &[#(#vein_component_stack),*],
                    vein_toggle: #vein_router_toggle,
                    vein_ridged: #vein_router_ridged,
                    vein_gap: #vein_router_gap,
                },
            }
        };

        (base_routers_ts, compiled_mod_ts)
    }
}

fn clean_noise_name(n: &str) -> String {
    n.strip_prefix("minecraft:").unwrap_or(n).to_string()
}

fn parse_axis(s: &str) -> Result<Axis, String> {
    Ok(match s {
        "x" | "X" => Axis::X,
        "y" | "Y" => Axis::Y,
        "z" | "Z" => Axis::Z,
        other => return Err(format!("unknown axis {other}")),
    })
}

fn parse_tiling(s: &str) -> Result<Tiling, String> {
    Ok(match s {
        "clamp_to_edge" => Tiling::ClampToEdge,
        "repeat" => Tiling::Repeat,
        "mirrored_repeat" => Tiling::MirroredRepeat,
        other => return Err(format!("unknown tiling {other}")),
    })
}

fn parse_metric(s: &str) -> Result<DistanceMetric, String> {
    Ok(match s {
        "euclidean" => DistanceMetric::Euclidean,
        "euclidean_squared" => DistanceMetric::EuclideanSquared,
        "manhattan" => DistanceMetric::Manhattan,
        "chebyshev" => DistanceMetric::Chebyshev,
        other => return Err(format!("unknown distance metric {other}")),
    })
}

struct DensityParser<'a> {
    densities: &'a BTreeMap<String, Value>,
    references: Vec<String>,
    depth: usize,
    remaining: usize,
}

impl DensityParser<'_> {
    fn visit<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.depth == 128 || self.remaining == 0 {
            return Err("density function expansion limit exceeded".into());
        }
        self.depth += 1;
        self.remaining -= 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn reference(&mut self, id: &str) -> Result<DensityFunctionRepr, String> {
        let id = if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        };
        let Some(value) = self.densities.get(&id) else {
            return match id.as_str() {
                "minecraft:y" => Ok(DensityFunctionRepr::Gradient {
                    data: GradientData {
                        axis: Axis::Y,
                        tiling: Tiling::ClampToEdge,
                        from_coordinate: -4064,
                        to_coordinate: 4062,
                        from_value: HashableF32(-4064.0),
                        to_value: HashableF32(4062.0),
                    },
                }),
                "minecraft:zero" => Ok(DensityFunctionRepr::Constant {
                    value: HashableF32(0.0),
                }),
                _ => Err(format!("unknown density function {id}")),
            };
        };
        if self.references.contains(&id) {
            return Err(format!("cyclic density function reference {id}"));
        }
        self.references.push(id);
        let result = self.density(value);
        self.references.pop();
        result
    }

    fn density(&mut self, val: &Value) -> Result<DensityFunctionRepr, String> {
        self.visit(|this| this.density_inner(val))
    }

    #[expect(clippy::too_many_lines, reason = "one codec branch per density node")]
    fn density_inner(&mut self, val: &Value) -> Result<DensityFunctionRepr, String> {
        Ok(match val {
            serde_json::Value::Number(n) => DensityFunctionRepr::Constant {
                value: HashableF32(n.as_f64().unwrap_or(0.0) as f32),
            },
            Value::String(id) => return self.reference(id),
            serde_json::Value::Object(obj) => {
                let type_str = obj
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or("density function object is missing type")?;
                let clean_type = type_str.strip_prefix("minecraft:").unwrap_or(type_str);

                match clean_type {
                    "constant" => {
                        let num = obj
                            .get("value")
                            .or_else(|| obj.get("argument"))
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        DensityFunctionRepr::Constant {
                            value: HashableF32((num) as f32),
                        }
                    }
                    "y_clamped_gradient" => {
                        let from_y =
                            obj.get("from_y").and_then(Value::as_i64).unwrap_or(-64) as i32;
                        let to_y = obj.get("to_y").and_then(Value::as_i64).unwrap_or(320) as i32;
                        let from_value =
                            obj.get("from_value").and_then(Value::as_f64).unwrap_or(0.0);
                        let to_value = obj.get("to_value").and_then(Value::as_f64).unwrap_or(0.0);
                        DensityFunctionRepr::Gradient {
                            data: GradientData {
                                axis: Axis::Y,
                                tiling: Tiling::ClampToEdge,
                                from_coordinate: from_y,
                                to_coordinate: to_y,
                                from_value: HashableF32((from_value) as f32),
                                to_value: HashableF32((to_value) as f32),
                            },
                        }
                    }
                    "gradient" => {
                        let axis_str = obj.get("axis").and_then(Value::as_str).unwrap_or("y");
                        let axis = parse_axis(axis_str)?;
                        let tiling_str = obj
                            .get("tiling")
                            .and_then(Value::as_str)
                            .unwrap_or("clamp_to_edge");
                        let tiling = parse_tiling(tiling_str)?;
                        let from_coordinate = obj
                            .get("from_coordinate")
                            .or_else(|| obj.get("from_y"))
                            .and_then(Value::as_i64)
                            .ok_or("Missing from_coordinate")?
                            as i32;
                        let to_coordinate =
                            obj.get("to_coordinate")
                                .or_else(|| obj.get("to_y"))
                                .and_then(Value::as_i64)
                                .ok_or("Missing to_coordinate")? as i32;
                        let from_value = obj
                            .get("from_value")
                            .and_then(Value::as_f64)
                            .ok_or("Missing from_value")?;
                        let to_value = obj
                            .get("to_value")
                            .and_then(Value::as_f64)
                            .ok_or("Missing to_value")?;
                        DensityFunctionRepr::Gradient {
                            data: GradientData {
                                axis,
                                tiling,
                                from_coordinate,
                                to_coordinate,
                                from_value: HashableF32((from_value) as f32),
                                to_value: HashableF32((to_value) as f32),
                            },
                        }
                    }
                    "distance_to_point" => {
                        let point_arr = obj
                            .get("point")
                            .and_then(Value::as_array)
                            .ok_or("Missing point")?;
                        let point: [i32; 3] =
                            serde_json::from_value(Value::Array(point_arr.clone()))
                                .map_err(|err| err.to_string())?;
                        let metric_str = obj
                            .get("metric")
                            .and_then(Value::as_str)
                            .unwrap_or("euclidean");
                        let metric = parse_metric(metric_str)?;
                        DensityFunctionRepr::DistanceToPoint {
                            data: DistanceToPointData { point, metric },
                        }
                    }
                    "lerp" => {
                        let alpha = self.density(obj.get("alpha").ok_or("Missing alpha")?)?;
                        let first = self.density(obj.get("first").ok_or("Missing first")?)?;
                        let second = self.density(obj.get("second").ok_or("Missing second")?)?;
                        DensityFunctionRepr::Lerp {
                            alpha: Box::new(alpha),
                            first: Box::new(first),
                            second: Box::new(second),
                        }
                    }
                    "floor" | "round" | "ceil" | "truncate" => {
                        let op = match clean_type {
                            "floor" => RoundingOperation::Floor,
                            "round" => RoundingOperation::Round,
                            "ceil" => RoundingOperation::Ceil,
                            "truncate" => RoundingOperation::Truncate,
                            _ => return Err(format!("unsupported operation {clean_type}")),
                        };
                        let input_node = obj
                            .get("input")
                            .or_else(|| obj.get("argument"))
                            .ok_or("Missing input")?;
                        let input = self.density(input_node)?;
                        let multiple = if let Some(m) = obj.get("multiple") {
                            self.density(m)?
                        } else {
                            DensityFunctionRepr::Constant {
                                value: HashableF32(1.0),
                            }
                        };
                        DensityFunctionRepr::Rounding {
                            input: Box::new(input),
                            multiple: Box::new(multiple),
                            data: RoundingData { operation: op },
                        }
                    }
                    "slice" => {
                        let axis_str = obj.get("axis").and_then(Value::as_str).unwrap_or("y");
                        let axis = parse_axis(axis_str)?;
                        let coordinate =
                            obj.get("coordinate")
                                .and_then(Value::as_i64)
                                .ok_or("Missing coordinate")? as i32;
                        let input = self.density(obj.get("input").ok_or("Missing input")?)?;
                        DensityFunctionRepr::Slice {
                            axis,
                            coordinate,
                            input: Box::new(input),
                        }
                    }
                    "old_blended_noise" => {
                        let xz_scale = obj.get("xz_scale").and_then(Value::as_f64).unwrap_or(0.25);
                        let y_scale = obj.get("y_scale").and_then(Value::as_f64).unwrap_or(0.125);
                        let xz_factor =
                            obj.get("xz_factor").and_then(Value::as_f64).unwrap_or(80.0);
                        let y_factor = obj.get("y_factor").and_then(Value::as_f64).unwrap_or(160.0);
                        let smear_scale_multiplier = obj
                            .get("smear_scale_multiplier")
                            .and_then(Value::as_f64)
                            .unwrap_or(8.0);
                        DensityFunctionRepr::InterpolatedNoiseSampler {
                            data: InterpolatedNoiseSamplerData {
                                xz_scale: HashableF64(xz_scale),
                                y_scale: HashableF64(y_scale),
                                xz_factor: HashableF64(xz_factor),
                                y_factor: HashableF64(y_factor),
                                smear_scale_multiplier: HashableF64(smear_scale_multiplier),
                            },
                        }
                    }
                    "add" | "mul" | "min" | "max" | "sub" | "div" | "pow" => {
                        let arg1_node = obj
                            .get("left")
                            .or_else(|| obj.get("argument1"))
                            .or_else(|| obj.get("base"))
                            .ok_or("Missing left/argument1/base")?;
                        let arg2_node = obj
                            .get("right")
                            .or_else(|| obj.get("argument2"))
                            .or_else(|| obj.get("exponent"))
                            .ok_or("Missing right/argument2/exponent")?;
                        let arg1 = self.density(arg1_node)?;
                        let arg2 = self.density(arg2_node)?;
                        let min_value = obj
                            .get("min_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::NEG_INFINITY);
                        let max_value = obj
                            .get("max_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::INFINITY);

                        if clean_type == "add" || clean_type == "mul" {
                            let linear_op = if clean_type == "add" {
                                LinearOperation::Add
                            } else {
                                LinearOperation::Mul
                            };

                            if let DensityFunctionRepr::Constant { value: c } = arg1 {
                                return Ok(DensityFunctionRepr::Linear {
                                    input: Box::new(arg2),
                                    data: LinearData {
                                        operation: linear_op,
                                        argument: c,
                                        min_value: HashableF32((min_value) as f32),
                                        max_value: HashableF32((max_value) as f32),
                                    },
                                });
                            } else if let DensityFunctionRepr::Constant { value: c } = arg2 {
                                return Ok(DensityFunctionRepr::Linear {
                                    input: Box::new(arg1),
                                    data: LinearData {
                                        operation: linear_op,
                                        argument: c,
                                        min_value: HashableF32((min_value) as f32),
                                        max_value: HashableF32((max_value) as f32),
                                    },
                                });
                            }
                        }

                        let op = match clean_type {
                            "add" => BinaryOperation::Add,
                            "mul" => BinaryOperation::Mul,
                            "min" => BinaryOperation::Min,
                            "max" => BinaryOperation::Max,
                            "sub" => BinaryOperation::Sub,
                            "div" => BinaryOperation::Div,
                            "pow" => BinaryOperation::Pow,
                            _ => return Err(format!("unsupported operation {clean_type}")),
                        };

                        DensityFunctionRepr::Binary {
                            argument1: Box::new(arg1),
                            argument2: Box::new(arg2),
                            data: BinaryData {
                                operation: op,
                                min_value: HashableF32((min_value) as f32),
                                max_value: HashableF32((max_value) as f32),
                            },
                        }
                    }
                    "abs" | "square" | "cube" | "half_negative" | "quarter_negative"
                    | "squeeze" | "invert" | "reciprocal" | "negate" | "sqrt" | "log" | "sign" => {
                        let op = match clean_type {
                            "abs" => UnaryOperation::Abs,
                            "square" => UnaryOperation::Square,
                            "cube" => UnaryOperation::Cube,
                            "half_negative" => UnaryOperation::HalfNegative,
                            "quarter_negative" => UnaryOperation::QuarterNegative,
                            "squeeze" => UnaryOperation::Squeeze,
                            "invert" | "reciprocal" => UnaryOperation::Invert,
                            "negate" => UnaryOperation::Negate,
                            "sqrt" => UnaryOperation::Sqrt,
                            "log" => UnaryOperation::Log,
                            "sign" => UnaryOperation::Sign,
                            _ => return Err(format!("unsupported operation {clean_type}")),
                        };
                        let input_node = obj
                            .get("argument")
                            .or_else(|| obj.get("input"))
                            .ok_or("Missing argument/input")?;
                        let input = self.density(input_node)?;
                        let min_value = obj
                            .get("min_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::NEG_INFINITY);
                        let max_value = obj
                            .get("max_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::INFINITY);
                        DensityFunctionRepr::Unary {
                            input: Box::new(input),
                            data: UnaryData {
                                operation: op,
                                min_value: HashableF32((min_value) as f32),
                                max_value: HashableF32((max_value) as f32),
                            },
                        }
                    }
                    "clamp" => {
                        let input_node = obj
                            .get("input")
                            .or_else(|| obj.get("argument"))
                            .ok_or("Missing input")?;
                        let input = self.density(input_node)?;
                        let min_val = obj
                            .get("min")
                            .or_else(|| obj.get("min_value"))
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::NEG_INFINITY);
                        let max_val = obj
                            .get("max")
                            .or_else(|| obj.get("max_value"))
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::INFINITY);
                        if min_val > max_val {
                            return Err("clamp minimum exceeds maximum".into());
                        }
                        DensityFunctionRepr::Clamp {
                            input: Box::new(input),
                            data: ClampData {
                                min_value: HashableF32((min_val) as f32),
                                max_value: HashableF32((max_val) as f32),
                            },
                        }
                    }
                    "range_choice" => {
                        let input = self.density(obj.get("input").ok_or("Missing input")?)?;
                        let min_inc = obj
                            .get("min_inclusive")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::NEG_INFINITY);
                        let max_exc = obj
                            .get("max_exclusive")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::INFINITY);
                        let when_in =
                            self.density(obj.get("when_in_range").ok_or("Missing when_in_range")?)?;
                        let when_out = self.density(
                            obj.get("when_out_of_range")
                                .ok_or("Missing when_out_of_range")?,
                        )?;
                        DensityFunctionRepr::RangeChoice {
                            input: Box::new(input),
                            when_in_range: Box::new(when_in),
                            when_out_range: Box::new(when_out),
                            data: RangeChoiceData {
                                min_inclusive: HashableF32((min_inc) as f32),
                                max_exclusive: HashableF32((max_exc) as f32),
                            },
                        }
                    }
                    "interval_select" => {
                        let input = self.density(obj.get("input").ok_or("Missing input")?)?;
                        let thresholds: Vec<HashableF32> = obj
                            .get("thresholds")
                            .and_then(Value::as_array)
                            .ok_or("Missing thresholds array")?
                            .iter()
                            .map(|v| HashableF32(v.as_f64().unwrap_or(0.0) as f32))
                            .collect();
                        let funcs: Vec<DensityFunctionRepr> = obj
                            .get("functions")
                            .and_then(Value::as_array)
                            .ok_or("Missing functions array")?
                            .iter()
                            .map(|v| self.density(v))
                            .collect::<Result<_, _>>()?;

                        if funcs.len() != thresholds.len() + 1
                            || !thresholds.windows(2).all(|pair| pair[0].0 < pair[1].0)
                        {
                            return Err("interval_select requires ordered thresholds and one function per interval".into());
                        }
                        DensityFunctionRepr::IntervalSelect {
                            input: Box::new(input),
                            thresholds: thresholds.into_boxed_slice(),
                            functions: funcs.into_boxed_slice(),
                        }
                    }
                    "spline" => {
                        let spline_node = obj.get("spline").ok_or("Missing spline")?;
                        let spline = self.spline(spline_node)?;
                        let min_value = obj
                            .get("min_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::NEG_INFINITY);
                        let max_value = obj
                            .get("max_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(f64::INFINITY);
                        DensityFunctionRepr::Spline {
                            spline,
                            data: SplineData {
                                min_value: HashableF32((min_value) as f32),
                                max_value: HashableF32((max_value) as f32),
                            },
                        }
                    }
                    "noise" => {
                        let noise_name = obj
                            .get("noise")
                            .and_then(Value::as_str)
                            .ok_or("Missing noise name")?;
                        let xz_scale = obj.get("xz_scale").and_then(Value::as_f64).unwrap_or(1.0);
                        let y_scale = obj.get("y_scale").and_then(Value::as_f64).unwrap_or(1.0);
                        let zero = Value::from(0);

                        if obj.contains_key("shift_x")
                            || obj.contains_key("shift_y")
                            || obj.contains_key("shift_z")
                        {
                            let shift_x = self.density(obj.get("shift_x").unwrap_or(&zero))?;
                            let shift_y = self.density(obj.get("shift_y").unwrap_or(&zero))?;
                            let shift_z = self.density(obj.get("shift_z").unwrap_or(&zero))?;
                            DensityFunctionRepr::ShiftedNoise {
                                shift_x: Box::new(shift_x),
                                shift_y: Box::new(shift_y),
                                shift_z: Box::new(shift_z),
                                data: ShiftedNoiseData {
                                    noise_id: clean_noise_name(noise_name),
                                    xz_scale: HashableF64(xz_scale),
                                    y_scale: HashableF64(y_scale),
                                },
                            }
                        } else {
                            DensityFunctionRepr::Noise {
                                data: NoiseData {
                                    noise_id: clean_noise_name(noise_name),
                                    xz_scale: HashableF64(xz_scale),
                                    y_scale: HashableF64(y_scale),
                                },
                            }
                        }
                    }
                    "shifted_noise" => {
                        let shift_x = self.density(obj.get("shift_x").ok_or("Missing shift_x")?)?;
                        let shift_y = self.density(obj.get("shift_y").ok_or("Missing shift_y")?)?;
                        let shift_z = self.density(obj.get("shift_z").ok_or("Missing shift_z")?)?;
                        let noise_name = obj
                            .get("noise")
                            .and_then(Value::as_str)
                            .ok_or("Missing noise name")?;
                        let xz_scale = obj.get("xz_scale").and_then(Value::as_f64).unwrap_or(1.0);
                        let y_scale = obj.get("y_scale").and_then(Value::as_f64).unwrap_or(1.0);
                        DensityFunctionRepr::ShiftedNoise {
                            shift_x: Box::new(shift_x),
                            shift_y: Box::new(shift_y),
                            shift_z: Box::new(shift_z),
                            data: ShiftedNoiseData {
                                noise_id: clean_noise_name(noise_name),
                                xz_scale: HashableF64(xz_scale),
                                y_scale: HashableF64(y_scale),
                            },
                        }
                    }
                    "shift_a" => {
                        let offset_noise = obj
                            .get("noise")
                            .or_else(|| obj.get("argument"))
                            .and_then(Value::as_str)
                            .ok_or("Missing noise")?;
                        DensityFunctionRepr::ShiftA {
                            noise_id: clean_noise_name(offset_noise),
                        }
                    }
                    "shift_b" => {
                        let offset_noise = obj
                            .get("noise")
                            .or_else(|| obj.get("argument"))
                            .and_then(Value::as_str)
                            .ok_or("Missing noise")?;
                        DensityFunctionRepr::ShiftB {
                            noise_id: clean_noise_name(offset_noise),
                        }
                    }
                    "shift" => {
                        let offset_noise = obj
                            .get("noise")
                            .or_else(|| obj.get("argument"))
                            .and_then(Value::as_str)
                            .ok_or("Missing noise")?;
                        DensityFunctionRepr::ShiftA {
                            noise_id: clean_noise_name(offset_noise),
                        }
                    }
                    "blend_alpha" => DensityFunctionRepr::BlendAlpha,
                    "blend_offset" => DensityFunctionRepr::BlendOffset,
                    "blend_density" => {
                        let input = self.density(
                            obj.get("input")
                                .or_else(|| obj.get("argument"))
                                .ok_or("Missing input/argument")?,
                        )?;
                        DensityFunctionRepr::BlendDensity {
                            input: Box::new(input),
                        }
                    }
                    "end_islands" | "end_outer_islands" => DensityFunctionRepr::EndIslands,
                    "beardifier" => DensityFunctionRepr::Beardifier,
                    "cache" | "interpolated" | "flat_cache" | "cache_flat" | "cache_2d"
                    | "cache_once" | "cache_all_in_cell" => {
                        let input = self.density(
                            obj.get("input")
                                .or_else(|| obj.get("argument"))
                                .ok_or("Missing input/argument")?,
                        )?;
                        let wrapper = if clean_type == "interpolated" {
                            let cell_size_xz = obj
                                .get("cell_size_xz")
                                .and_then(Value::as_i64)
                                .and_then(|value| i32::try_from(value).ok())
                                .filter(|&size| size > 0)
                                .ok_or("cell_size_xz must be a positive integer")?;
                            let cell_size_y = obj
                                .get("cell_size_y")
                                .and_then(Value::as_i64)
                                .and_then(|value| i32::try_from(value).ok())
                                .filter(|&size| size > 0)
                                .ok_or("cell_size_y must be a positive integer")?;
                            WrapperType::Interpolated {
                                cell_size_xz,
                                cell_size_y,
                            }
                        } else {
                            WrapperType::Cache
                        };
                        DensityFunctionRepr::Wrapper {
                            input: Box::new(input),
                            wrapper,
                        }
                    }
                    "find_top_surface" | "weird_utility_density" => {
                        let density = self.density(obj.get("density").ok_or("Missing density")?)?;
                        let upper_bound =
                            self.density(obj.get("upper_bound").ok_or("Missing upper_bound")?)?;
                        let lower_bound = obj
                            .get("lower_bound")
                            .and_then(Value::as_i64)
                            .unwrap_or(-64) as i32;
                        let cell_height =
                            obj.get("cell_height").and_then(Value::as_i64).unwrap_or(8) as i32;
                        if cell_height <= 0 {
                            return Err("find_top_surface cell_height must be positive".into());
                        }
                        DensityFunctionRepr::FindTopSurface {
                            density: Box::new(density),
                            upper_bound: Box::new(upper_bound),
                            data: FindTopSurfaceData {
                                lower_bound,
                                cell_height,
                            },
                        }
                    }
                    other => return Err(format!("unsupported density function type {other}")),
                }
            }
            _ => return Err("density function must be a number, resource ID or object".into()),
        })
    }

    fn spline(&mut self, val: &Value) -> Result<SplineRepr, String> {
        self.visit(|this| this.spline_inner(val))
    }

    fn spline_inner(&mut self, val: &Value) -> Result<SplineRepr, String> {
        if let Some(n) = val.as_f64() {
            return Ok(SplineRepr::Fixed {
                value: HashableF32(n as f32),
            });
        }
        if let Some(obj) = val.as_object() {
            if obj.contains_key("value") && !obj.contains_key("points") {
                return self.spline(&obj["value"]);
            }
            let loc_fn = self.density(
                obj.get("coordinate")
                    .ok_or("Missing coordinate in spline")?,
            )?;
            let points = obj
                .get("points")
                .and_then(Value::as_array)
                .ok_or("Missing points array in spline")?;

            if points.is_empty() {
                return Err("spline has no points".into());
            }
            let mut locations = Vec::new();
            let mut values = Vec::new();
            let mut derivatives = Vec::new();

            for pt in points {
                locations.push(HashableF32(
                    pt["location"].as_f64().ok_or("invalid spline location")? as f32,
                ));
                values.push(self.spline(&pt["value"])?);
                derivatives.push(HashableF32(
                    pt["derivative"]
                        .as_f64()
                        .ok_or("invalid spline derivative")? as f32,
                ));
            }

            if !locations.windows(2).all(|pair| pair[0].0 <= pair[1].0) {
                return Err("spline locations must be in ascending order".into());
            }
            return Ok(SplineRepr::Standard {
                location_function: Box::new(loc_fn),
                locations: locations.into_boxed_slice(),
                values: values.into_boxed_slice(),
                derivatives: derivatives.into_boxed_slice(),
            });
        }
        Err("spline must be a number or object".into())
    }
}

/// Resolves all density functions referenced by a noise settings entry.
pub fn parse_noise_router(
    dim_name: &str,
    val: &Value,
    densities: &BTreeMap<String, Value>,
) -> Result<NoiseRouterRepr, String> {
    let mut parser = DensityParser {
        densities,
        references: Vec::new(),
        depth: 0,
        remaining: 262_144,
    };
    let nr = val
        .get("noise_router")
        .and_then(Value::as_object)
        .ok_or("noise_router must be an object")?;
    let aquifers = val.get("aquifers");

    let zero = Value::from(0);

    let barrier = nr
        .get("barrier")
        .or_else(|| aquifers.and_then(|a| a.get("barrier")))
        .unwrap_or(&zero);
    let fluid_level_floodedness = nr
        .get("fluid_level_floodedness")
        .or_else(|| aquifers.and_then(|a| a.get("fluid_level_floodedness")))
        .unwrap_or(&zero);
    let fluid_level_spread = nr
        .get("fluid_level_spread")
        .or_else(|| aquifers.and_then(|a| a.get("fluid_level_spread")))
        .unwrap_or(&zero);
    let lava = nr
        .get("lava")
        .or_else(|| aquifers.and_then(|a| a.get("lava")))
        .unwrap_or(&zero);
    let preliminary_surface_level = nr
        .get("preliminary_surface_level")
        .or_else(|| nr.get("chunk_surface_level"))
        .or_else(|| aquifers.and_then(|a| a.get("surface_level")))
        .unwrap_or(&zero);

    let vein_toggle = nr.get("vein_toggle").cloned().unwrap_or_else(|| {
        if dim_name.starts_with("overworld")
            || dim_name == "amplified"
            || dim_name == "large_biomes"
        {
            serde_json::Value::String("minecraft:overworld/ore_vein/toggle".to_string())
        } else {
            zero.clone()
        }
    });
    let vein_ridged = nr.get("vein_ridged").cloned().unwrap_or_else(|| {
        if dim_name.starts_with("overworld")
            || dim_name == "amplified"
            || dim_name == "large_biomes"
        {
            serde_json::Value::String("minecraft:overworld/ore_vein/mask".to_string())
        } else {
            zero.clone()
        }
    });
    let vein_gap = nr.get("vein_gap").cloned().unwrap_or_else(|| {
        if dim_name.starts_with("overworld")
            || dim_name == "amplified"
            || dim_name == "large_biomes"
        {
            serde_json::Value::String("minecraft:overworld/ore_vein/gap".to_string())
        } else {
            zero.clone()
        }
    });

    Ok(NoiseRouterRepr {
        barrier_noise: parser.density(barrier)?,
        fluid_level_floodedness_noise: parser.density(fluid_level_floodedness)?,
        fluid_level_spread_noise: parser.density(fluid_level_spread)?,
        lava_noise: parser.density(lava)?,
        temperature: parser.density(nr.get("temperature").unwrap_or(&zero))?,
        vegetation: parser.density(nr.get("vegetation").unwrap_or(&zero))?,
        continents: parser.density(nr.get("continents").unwrap_or(&zero))?,
        erosion: parser.density(nr.get("erosion").unwrap_or(&zero))?,
        depth: parser.density(nr.get("depth").unwrap_or(&zero))?,
        ridges: parser.density(nr.get("ridges").unwrap_or(&zero))?,
        preliminary_surface_level: parser.density(preliminary_surface_level)?,
        final_density: parser.density(nr.get("final_density").unwrap_or(&zero))?,
        vein_toggle: parser.density(&vein_toggle)?,
        vein_ridged: parser.density(&vein_ridged)?,
        vein_gap: parser.density(&vein_gap)?,
    })
}

impl NoiseRouterRepr {
    fn visit_roots<S, E>(
        &self,
        [noise, surface, multinoise, vein]: [&mut S; 4],
        mut index: impl FnMut(&DensityFunctionRepr, &mut S) -> Result<usize, E>,
    ) -> Result<RouterRoots, E> {
        index(&self.preliminary_surface_level, surface)?;
        Ok(RouterRoots {
            noise_erosion: index(&self.erosion, noise)?,
            noise_depth: index(&self.depth, noise)?,
            barrier_noise: index(&self.barrier_noise, noise)?,
            fluid_level_floodedness_noise: index(&self.fluid_level_floodedness_noise, noise)?,
            fluid_level_spread_noise: index(&self.fluid_level_spread_noise, noise)?,
            lava_noise: index(&self.lava_noise, noise)?,
            final_density: index(&self.final_density, noise)?,
            vein_toggle: index(&self.vein_toggle, noise)?,
            vein_ridged: index(&self.vein_ridged, noise)?,
            vein_gap: index(&self.vein_gap, noise)?,
            ridges: index(&self.ridges, multinoise)?,
            temperature: index(&self.temperature, multinoise)?,
            vegetation: index(&self.vegetation, multinoise)?,
            continents: index(&self.continents, multinoise)?,
            multi_erosion: index(&self.erosion, multinoise)?,
            multi_depth: index(&self.depth, multinoise)?,
            vein_router_toggle: index(&self.vein_toggle, vein)?,
            vein_router_ridged: index(&self.vein_ridged, vein)?,
            vein_router_gap: index(&self.vein_gap, vein)?,
        })
    }
}

struct RouterRoots {
    noise_erosion: usize,
    noise_depth: usize,
    barrier_noise: usize,
    fluid_level_floodedness_noise: usize,
    fluid_level_spread_noise: usize,
    lava_noise: usize,
    final_density: usize,
    vein_toggle: usize,
    vein_ridged: usize,
    vein_gap: usize,
    ridges: usize,
    temperature: usize,
    vegetation: usize,
    continents: usize,
    multi_erosion: usize,
    multi_depth: usize,
    vein_router_toggle: usize,
    vein_router_ridged: usize,
    vein_router_gap: usize,
}

#[cfg(feature = "runtime")]
mod runtime_builder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_can_have_equal_coordinates() {
        let settings = serde_json::json!({"noise_router": {
            "final_density": {
                "type": "minecraft:spline",
                "spline": {
                    "coordinate": 0,
                    "points": [
                        {"location": 0, "value": 0, "derivative": 0},
                        {"location": 0, "value": 1, "derivative": 0}
                    ]
                }
            }
        }});
        assert!(parse_noise_router("test:settings", &settings, &BTreeMap::new()).is_ok());
    }

    #[test]
    fn cyclic_density_references_are_errors() {
        let settings = serde_json::json!({"noise_router": {"final_density": "test:a"}});
        let densities = BTreeMap::from([
            ("test:a".into(), Value::from("test:b")),
            ("test:b".into(), Value::from("test:a")),
        ]);
        let error = parse_noise_router("test:settings", &settings, &densities).err();
        assert_eq!(
            error.as_deref(),
            Some("cyclic density function reference test:a")
        );
    }
}
