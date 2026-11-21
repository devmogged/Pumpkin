use std::{collections::BTreeMap, fs};

use proc_macro2::TokenStream;
use pumpkin_util::DoublePerlinNoiseParametersCodec;
use pumpkin_worldgen_model::noise_parameter::NoiseParameterJson;
use quote::{format_ident, quote};

fn collect_noise_files(
    base: &std::path::Path,
    dir: &std::path::Path,
    result: &mut BTreeMap<String, DoublePerlinNoiseParametersCodec>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_noise_files(base, &path, result);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let rel = path.strip_prefix(base).unwrap();
            let rel_str = rel
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let key = format!("minecraft:{rel_str}");
            let content = fs::read_to_string(&path).expect("Failed to read noise file");
            let (first_octave, amplitudes) = serde_json::from_str::<NoiseParameterJson>(&content)
                .expect("Failed to parse noise parameter JSON")
                .into_parameters()
                .expect("Invalid vanilla noise parameters");
            let param = DoublePerlinNoiseParametersCodec {
                first_octave,
                amplitudes,
            };
            result.insert(key, param);
        }
    }
}

pub fn build() -> TokenStream {
    let dir = std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/noise");
    let mut json: BTreeMap<String, DoublePerlinNoiseParametersCodec> = BTreeMap::new();
    collect_noise_files(dir, dir, &mut json);

    let mut variants = TokenStream::new();
    let mut match_variants = TokenStream::new();

    for (i, (raw_name, parameter)) in json.iter().enumerate() {
        let name = raw_name
            .strip_prefix("minecraft:")
            .unwrap()
            .replace("/", "_");
        let name_ident = format_ident!("{}", name.to_uppercase());

        // 1. MD5 Pre-calculation
        let hash = md5::compute(raw_name.as_bytes());
        let lo = u64::from_be_bytes(hash[0..8].try_into().unwrap());
        let hi = u64::from_be_bytes(hash[8..16].try_into().unwrap());

        let string_hash = pumpkin_util::math::java_string_hash(raw_name);

        let amplitudes = &parameter.amplitudes;
        let first_octave = parameter.first_octave;

        variants.extend([quote! {
            pub const #name_ident: DoublePerlinNoiseParameters = DoublePerlinNoiseParameters::new(
                #i,
                #first_octave,
                &[#(#amplitudes),*],
                #lo,
                #hi,
                #string_hash,
            );
        }]);

        match_variants.extend([quote! {
            #name => &Self::#name_ident,
        }]);
    }
    let count = json.len();

    quote! {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct DoublePerlinNoiseParameters {
            pub id: usize,
            pub first_octave: i32,
            pub amplitudes: &'static [f64],
            pub lo: u64,
            pub hi: u64,
            pub string_hash: i32,
            pub normalization: Option<pumpkin_util::noise::perlin::NormalNoiseSettings>,
        }

        impl DoublePerlinNoiseParameters {
            pub const COUNT: usize = #count;

            pub const fn new(
                id: usize,
                first_octave: i32,
                amplitudes: &'static [f64],
                lo: u64,
                hi: u64,
                string_hash: i32,
            ) -> Self {
                Self { id, first_octave, amplitudes, lo, hi, string_hash, normalization: None }
            }

            #[must_use]
            pub const fn with_normalization(
                mut self,
                normalization: pumpkin_util::noise::perlin::NormalNoiseSettings,
            ) -> Self {
                self.normalization = Some(normalization);
                self
            }

            pub fn id_to_parameters(id: &str) -> Option<&'static DoublePerlinNoiseParameters> {
                let id = id.strip_prefix("minecraft:").unwrap_or(id).replace("/", "_");
                Some(match id.as_str() {
                    #match_variants
                    _ => return None,
                })
            }

            #variants
        }
    }
}
