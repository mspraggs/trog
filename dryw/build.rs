// build.rs

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let man_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let src_path = Path::new(&man_dir).join("src/core.dryw");

    let core_source = fs::read_to_string(src_path)
        .unwrap()
        .as_str()
        .replace("\"", "\\\"");

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("core.dryw.rs");
    fs::write(
        &dest_path,
        format!("const CORE_SOURCE: &str = \"{}\";", core_source).as_str(),
    )
    .unwrap();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/core.dryw");
}
