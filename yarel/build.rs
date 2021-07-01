/* Copyright 2021 Matt Spraggs
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde::Serialize;
use tera::{Context, Tera};
use yaml_rust::YamlLoader;

const REPLACE_STRINGS: &[&str] = &[".", "/", "-"];

#[derive(Clone, Copy, Debug, Serialize)]
enum ClassKind {
    NativeValue,
    NativeObject,
    Yarel,
}

impl From<&str> for ClassKind {
    fn from(value: &str) -> Self {
        match value {
            "native_value" => Self::NativeValue,
            "native_object" => Self::NativeObject,
            "yarel" => Self::Yarel,
            _ => {
                panic!("Unknown class kind.")
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct ClassSpec {
    name: String,
    repr: String,
    kind: ClassKind,
    superclass: String,
    metaclass: String,
}

fn main() {
    let man_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir);

    // Compile core classes file.
    compile_core_classes(&man_dir, out_dir);

    // Compile class store.
    compile_class_store(&man_dir, out_dir);

    // Compile tests.
    compile_tests(&man_dir, out_dir);

    println!("cargo:rerun-if-changed=build.rs");
}

fn compile_core_classes(man_dir: &OsString, out_dir: &Path) {
    let src_path = Path::new(&man_dir).join("src/core.yl");

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
    println!("cargo:rerun-if-changed=src/core.yl");
}

fn compile_class_store(man_dir: &OsString, out_dir: &Path) {
    let spec_path = Path::new(&man_dir).join("src/class_store.yaml");
    let template_path = Path::new(&man_dir).join("src/class_store.template.rs");

    let yaml_raw = fs::read_to_string(spec_path).unwrap();
    let yaml = YamlLoader::load_from_str(&yaml_raw).unwrap();

    let num_classes = yaml.len();
    fs::write("/tmp/debug.log", format!("{}\n", num_classes)).unwrap();
    let class_specs = yaml[0]
        .as_vec()
        .unwrap()
        .iter()
        .map(|y| {
            let name = y["name"].as_str().unwrap().to_owned();
            let repr = y["repr"]
                .as_str()
                .map(|s| s.to_owned())
                .unwrap_or_else(|| to_capcase(&name));
            let kind = y["kind"].as_str().map(|s| ClassKind::from(s)).unwrap();
            let superclass = y["superclass"].as_str().unwrap_or("object").to_owned();
            let metaclass = y["metaclass"]
                .as_str()
                .unwrap_or("base_metaclass")
                .to_owned();
            ClassSpec {
                name: if name.ends_with("class") {
                    name
                } else {
                    format!("{}_class", name)
                },
                repr,
                kind,
                superclass,
                metaclass,
            }
        })
        .collect::<Vec<_>>();

    let dest_path = out_dir.join("class_store.yaml.rs");

    let template = fs::read_to_string(template_path).unwrap();
    let mut context = Context::new();
    context.insert("class_specs", &class_specs);
    let result = Tera::one_off(&template, &context, false).unwrap();
    fs::write(dest_path, result).unwrap();

    println!("cargo:rerun-if-changed=src/class_store.yaml");
    println!("cargo:rerun-if-changed=src/class_store.template.rs");
}

fn compile_tests(man_dir: &OsString, out_dir: &Path) {
    let tests_dir = Path::new(&man_dir).join("tests");
    let tests_path = tests_dir.join("scripts");
    let paths = get_paths(&tests_path, Some(".yl")).unwrap();

    let compiled_tests_path = out_dir.join("compiled_tests.rs");
    let test_code = generate_tests(&tests_path, &paths);
    fs::write(compiled_tests_path, test_code).unwrap();

    let module_loader_path = out_dir.join("module_loader.rs");
    let modules_map = generate_module_loader(&tests_path, &paths);
    fs::write(module_loader_path, modules_map).unwrap();

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

fn to_capcase(s: &str) -> String {
    let mut ret = String::new();
    let mut observed_underscore = true;

    for c in s.chars() {
        if c == '_' {
            observed_underscore = true;
        } else if observed_underscore {
            ret.push(c.to_ascii_uppercase());
            observed_underscore = false;
        } else {
            ret.push(c.to_ascii_lowercase());
        }
    }

    ret
}
