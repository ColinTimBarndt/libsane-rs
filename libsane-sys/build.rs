use bindgen::{
    callbacks::{EnumVariantValue, IntKind, ParseCallbacks},
    EnumVariation,
};
use convert_case::{Case, Casing};
use std::path::PathBuf;

fn main() {
    let bindings = bindgen::builder()
        .header("/usr/include/sane/sane.h")
        .default_enum_style(EnumVariation::NewType {
            is_bitfield: false,
            is_global: false,
        })
        .prepend_enum_name(false)
        .disable_name_namespacing()
        .disable_nested_struct_naming()
        .derive_debug(true)
        .derive_default(true)
        .parse_callbacks(Box::new(Callbacks))
        .c_naming(false)
        // Cannot be used anyways
        .blocklist_item("SANE_Auth_Data")
        .generate()
        .unwrap();

    bindings
        .write_to_file(
            [std::env::var("OUT_DIR").unwrap().as_str(), "sane.rs"]
                .iter()
                .collect::<PathBuf>(),
        )
        .unwrap();

    println!("cargo:rustc-link-lib=sane");
}

#[derive(Debug)]
struct Callbacks;

impl ParseCallbacks for Callbacks {
    fn enum_variant_name(
        &self,
        enum_name: Option<&str>,
        original_variant_name: &str,
        _variant_value: EnumVariantValue,
    ) -> Option<String> {
        match enum_name {
            Some("SANE_Value_Type") => {
                let name = original_variant_name.strip_prefix("SANE_TYPE_")?;
                let name = name.to_case(Case::UpperCamel);
                Some(name)
            }
            Some(enum_name) => {
                let enum_name = enum_name.strip_suffix("_Type").unwrap_or(enum_name);
                let enum_name_uppercase = enum_name.to_ascii_uppercase();
                let prefix = format!("{}_", enum_name_uppercase);
                let new_variant_name = original_variant_name
                    .strip_prefix(&prefix)
                    .unwrap_or(original_variant_name);
                Some(new_variant_name.to_case(Case::UpperCamel))
            }
            None => None,
        }
    }

    fn int_macro(&self, name: &str, _value: i64) -> Option<IntKind> {
        match name {
            "SANE_FALSE" | "SANE_TRUE" => Some(IntKind::Custom {
                name: "Bool",
                is_signed: true,
            }),
            _ => None,
        }
    }

    fn item_name(&self, original_item_name: &str) -> Option<String> {
        let original_item_name = original_item_name
            .strip_prefix("SANE_")
            .unwrap_or(original_item_name);
        if original_item_name.contains('_')
            && original_item_name.to_case(Case::Snake) != original_item_name
            && original_item_name.to_case(Case::UpperSnake) != original_item_name
        {
            return Some(original_item_name.replace('_', ""));
        }
        Some(original_item_name.to_string())
    }
}
