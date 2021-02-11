// build.rs

use std::env;
use std::fs;
use std::path::Path;

const REPLACE_STRINGS: &[&str] = &[".", "/", "-"];

fn main() {
    let man_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let src_path = Path::new(&man_dir).join("src/core.yl");
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir);

    // Compile core classes file.

    let core_source = fs::read_to_string(src_path)
        .unwrap()
        .as_str()
        .replace("\"", "\\\"");

    let dest_path = out_dir.join("core.yl.rs");
    fs::write(
        &dest_path,
        format!("const CORE_SOURCE: &str = \"{}\";", core_source).as_str(),
    )
    .unwrap();

    // Compile tests.

    let tests_dir = Path::new(&man_dir).join("tests");
    let tests_path = tests_dir.join("scripts");
    let paths = get_paths(&tests_path, Some(".yl")).unwrap();

    let compiled_tests_path = out_dir.join("compiled_tests.rs");
    let test_code = generate_tests(&tests_path, &paths);
    fs::write(compiled_tests_path, test_code).unwrap();

    let module_loader_path = out_dir.join("module_loader.rs");
    let modules_map = generate_module_loader(&tests_path, &paths);
    fs::write(module_loader_path, modules_map).unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/core.yl");
    println!("cargo:rerun-if-changed=tests/test.rs");
    for path in &paths {
        println!("cargo:rerun-if-changed={}", path);
    }
}

fn get_paths(root: &Path, suffix: Option<&str>) -> Result<Vec<String>, ()> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| ())? {
        let path = entry.map_err(|_| ())?.path();
        let path_as_str = path.as_path().to_str().ok_or(())?;
        if path.is_dir() {
            paths.extend_from_slice(&get_paths(&Path::new(path_as_str), suffix)?);
        } else if suffix.is_none() {
            paths.push(path_as_str.to_owned());
        } else if let Some(suffix) = suffix {
            if path_as_str.ends_with(suffix) {
                paths.push(path_as_str.to_owned());
            }
        }
    }
    Ok(paths)
}

fn generate_module_loader(root: &Path, paths: &[String]) -> String {
    let mapping = paths
        .iter()
        .map(|p| {
            let suffix = get_module_name(root, p);
            let source = load_source(p);
            format!("\"{}\" => \"{}\"", suffix, source)
        })
        .fold(String::new(), |a, b| format!("{},\n{}", a, b));

    format!("gen_module_loader!(\n    \"\" => \"\"{}\n);", mapping)
}

fn generate_tests(root: &Path, paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| {
            let mut name = get_module_name(root, p);
            let source = load_source(p);
            for &string in REPLACE_STRINGS {
                name = name.replace(string, "_");
            }
            format!("test_case!({}, \"{}\");", name, source)
        })
        .fold("".to_string(), |a, b| format!("{}\n{}", a, b))
}

fn get_module_name(root: &Path, path: &str) -> String {
    Path::new(path)
        .with_extension("")
        .strip_prefix(root)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn load_source(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap()
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
}
