use proc_macro2::TokenStream;
use pumpkin_worldgen_model::carver::CarverJson;
use quote::{format_ident, quote};
use std::{collections::BTreeMap, fs, path::Path};

pub fn build() -> TokenStream {
    let dir = Path::new("../../assets/datapack/data/minecraft/worldgen/carver");
    let mut carvers = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let json = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            let carver = CarverJson::parse(json)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            carvers.insert(name, carver);
        }
    }
    let mut carver_instances = Vec::new();
    let mut lookup = Vec::new();
    let mut sources = Vec::new();
    for (name, carver) in carvers {
        let ident = format_ident!("{}", name.to_uppercase());
        let id = format!("minecraft:{name}");
        let definition = carver.emit(&id);
        carver_instances.push(quote!(pub static #ident: CarverConfig = #definition;));
        lookup.push(quote!(#name => Some(&#ident),));
        let source =
            format!("../../../../assets/datapack/data/minecraft/worldgen/carver/{name}.json");
        sources.push(quote!((#id, include_str!(#source))));
    }

    quote! {
        use pumpkin_util::math::float_provider::{
            ClampedNormalFloatProvider, ConstantFloatProvider, FloatProvider, NormalFloatProvider,
            TrapezoidFloatProvider, UniformFloatProvider,
        };
        use pumpkin_util::math::int_provider::{
            BiasedToBottomIntProvider, ConstantIntProvider, IntProvider, NormalIntProvider,
            UniformIntProvider, VeryBiasedToBottomIntProvider,
        };
        use pumpkin_util::y_offset::{AboveBottom, Absolute, BelowTop, RelativeToSeaLevel, YOffset};

        #[derive(Clone, Debug)]
        pub enum HeightProvider {
            Constant(YOffset),
            Uniform(UniformHeightProvider),
            Trapezoid(TrapezoidHeightProvider),
            VeryBiasedToBottom(VeryBiasedToBottomHeightProvider),
        }

        #[derive(Clone, Debug)]
        pub struct UniformHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
        }

        #[derive(Clone, Debug)]
        pub struct TrapezoidHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
            pub plateau: Option<i32>,
        }

        #[derive(Clone, Debug)]
        pub struct VeryBiasedToBottomHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
            pub inner: Option<std::num::NonZero<u32>>,
        }

        #[derive(Clone, Debug)]
        pub struct CaveCarverConfig {
            pub count: IntProvider,
            pub horizontal_radius_multiplier: FloatProvider,
            pub vertical_radius_multiplier: FloatProvider,
            pub floor_level: FloatProvider,
            pub room_vertical_radius_multiplier: FloatProvider,
            pub start_vertical_radius_multiplier: FloatProvider,
            pub thickness: FloatProvider,
            pub weird_thickness_bias: bool,
        }

        impl CaveCarverConfig {
            #[must_use]
            pub const fn default() -> Self {
                Self {
                    count: IntProvider::Constant(1),
                    horizontal_radius_multiplier: FloatProvider::Constant(1.0),
                    vertical_radius_multiplier: FloatProvider::Constant(1.0),
                    floor_level: FloatProvider::Constant(-0.7),
                    room_vertical_radius_multiplier: FloatProvider::Constant(0.5),
                    start_vertical_radius_multiplier: FloatProvider::Constant(1.0),
                    thickness: FloatProvider::Constant(1.0),
                    weird_thickness_bias: false,
                }
            }
        }

        #[derive(Clone, Debug)]
        pub struct CanyonShapeConfig {
            pub distance_factor: FloatProvider,
            pub thickness: FloatProvider,
            pub width_smoothness: i32,
            pub horizontal_radius_factor: FloatProvider,
            pub vertical_radius_default_factor: f32,
            pub vertical_radius_center_factor: f32,
            pub y_scale: FloatProvider,
        }

        #[derive(Clone, Debug)]
        pub struct CanyonCarverConfig {
            pub vertical_rotation: FloatProvider,
            pub shape: CanyonShapeConfig,
        }

        #[derive(Clone, Debug)]
        pub enum CarverAdditionalConfig {
            Cave(CaveCarverConfig),
            Canyon(CanyonCarverConfig),
        }

        #[derive(Clone, Debug)]
        pub struct CarverConfig {
            pub registry_id: &'static str,
            pub probability: f32,
            pub y: HeightProvider,
            pub additional: CarverAdditionalConfig,
        }

        impl CarverConfig {
            #[must_use]
            pub fn from_name(name: &str) -> Option<&'static Self> {
                match name.strip_prefix("minecraft:").unwrap_or(name) {
                    #(#lookup)*
                    _ => None,
                }
            }
        }

        pub const VANILLA_CARVERS_JSON: &[(&str, &str)] = &[#(#sources),*];

        #(#carver_instances)*
    }
}
