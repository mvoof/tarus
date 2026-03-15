use lsp_server::config_reader::discover_generators;
use lsp_server::indexer::GeneratorKind;
use std::fs;
use std::path::PathBuf;

/// Create a temp directory with a unique name under std::env::temp_dir().
/// Returns the path. Caller should clean up with `fs::remove_dir_all`.
fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("tarus_cfg_test_{name}"));
    let _ = fs::remove_dir_all(&p);
    p
}

/// Create the minimal directory structure for discover_generators to work:
/// `<root>/src-tauri/tauri.conf.json`
fn setup_workspace(root: &std::path::Path) -> PathBuf {
    let src_tauri = root.join("src-tauri");
    fs::create_dir_all(&src_tauri).unwrap();
    fs::write(
        src_tauri.join("tauri.conf.json"),
        r#"{ "identifier": "com.test" }"#,
    )
    .unwrap();
    src_tauri
}

// ─── Specta ──────────────────────────────────────────────────────────────

#[test]
fn test_specta_single_line_export() {
    let root = tmp("specta_single");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
use tauri_specta::{Builder};
fn run() {
    builder.export(Typescript::default(), "../src/bindings.ts").unwrap();
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let specta = gens.iter().find(|g| g.kind == GeneratorKind::Specta);
    assert!(specta.is_some(), "should find specta generator");
    let g = specta.unwrap();
    assert!(!g.is_directory);
    assert!(
        g.output_path.ends_with("src/bindings.ts"),
        "path was: {}",
        g.output_path.display()
    );
}

#[test]
fn test_specta_multiline_export_with_nested_parens() {
    let root = tmp("specta_multi");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
use tauri_specta::{collect_commands, Builder};
fn run() {
    specta_builder
        .export(
            Typescript::default().bigint(BigIntExportBehavior::Number),
            "../src/types/specta/bindings.ts",
        )
        .expect("Failed to export specta bindings");
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let specta = gens.iter().find(|g| g.kind == GeneratorKind::Specta);
    assert!(specta.is_some(), "should find specta with multiline export");
    let g = specta.unwrap();
    assert!(
        g.output_path.ends_with("src/types/specta/bindings.ts"),
        "path was: {}",
        g.output_path.display()
    );
}

#[test]
fn test_specta_jsdoc_export() {
    let root = tmp("specta_jsdoc");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
use tauri_specta::{Builder};
fn run() {
    builder.export(JSDoc::default(), "../src/bindings.js").unwrap();
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let specta = gens.iter().find(|g| g.kind == GeneratorKind::Specta);
    assert!(specta.is_some(), "should find specta with .js path");
    assert!(specta.unwrap().output_path.ends_with("src/bindings.js"));
}

#[test]
fn test_specta_not_found_without_tauri_specta_import() {
    let root = tmp("specta_no_import");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
fn run() {
    something.export(Default::default(), "../src/bindings.ts").unwrap();
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    assert!(
        gens.iter().all(|g| g.kind != GeneratorKind::Specta),
        "should not find specta without tauri_specta in content"
    );
}

#[test]
fn test_specta_cfg_guarded_export() {
    let root = tmp("specta_cfg");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
use tauri_specta::{collect_commands, Builder};
fn run() {
    #[cfg(all(debug_assertions, feature = "specta"))]
    specta_builder
        .export(
            Typescript::default(),
            "../src/types/specta/output.ts",
        )
        .expect("Failed");
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let specta = gens.iter().find(|g| g.kind == GeneratorKind::Specta);
    assert!(
        specta.is_some(),
        "should find specta even behind #[cfg()] guard"
    );
}

// ─── ts-rs ───────────────────────────────────────────────────────────────

#[test]
fn test_ts_rs_plain_string_env() {
    let root = tmp("tsrs_plain");
    let _src_tauri = setup_workspace(&root);
    let cargo_dir = root.join(".cargo");
    fs::create_dir_all(&cargo_dir).unwrap();
    fs::write(
        cargo_dir.join("config.toml"),
        "[env]\nTS_RS_EXPORT_DIR = \"../src/types/ts-rs\"\n",
    )
    .unwrap();
    // Need ts-rs dep so it doesn't just pick up from Cargo.toml fallback
    // Actually plain string is enough — parse_ts_rs_export_dir handles it

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let tsrs = gens.iter().find(|g| g.kind == GeneratorKind::TsRs);
    assert!(tsrs.is_some(), "should find ts-rs from .cargo/config.toml");
    let g = tsrs.unwrap();
    assert!(g.is_directory);
    assert!(
        g.output_path.ends_with("src/types/ts-rs"),
        "path was: {}",
        g.output_path.display()
    );
}

#[test]
fn test_ts_rs_inline_table_relative() {
    let root = tmp("tsrs_relative");
    let _src_tauri = setup_workspace(&root);
    let cargo_dir = root.join(".cargo");
    fs::create_dir_all(&cargo_dir).unwrap();
    fs::write(
        cargo_dir.join("config.toml"),
        "[env]\nTS_RS_EXPORT_DIR = { value = \"./src/bindings_type\", relative = true }\n",
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let tsrs = gens.iter().find(|g| g.kind == GeneratorKind::TsRs);
    assert!(tsrs.is_some(), "should find ts-rs with relative = true");
    let g = tsrs.unwrap();
    // relative = true → base is parent of .cargo/ dir = root
    assert!(
        g.output_path.ends_with("src/bindings_type"),
        "path was: {}",
        g.output_path.display()
    );
}

#[test]
fn test_ts_rs_src_tauri_cargo_config() {
    let root = tmp("tsrs_src_tauri_config");
    let src_tauri = setup_workspace(&root);
    let cargo_dir = src_tauri.join(".cargo");
    fs::create_dir_all(&cargo_dir).unwrap();
    fs::write(
        cargo_dir.join("config.toml"),
        "[env]\nTS_RS_EXPORT_DIR = \"../src/types/ts-rs\"\n",
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let tsrs = gens.iter().find(|g| g.kind == GeneratorKind::TsRs);
    assert!(
        tsrs.is_some(),
        "should find ts-rs from src-tauri/.cargo/config.toml"
    );
}

#[test]
fn test_ts_rs_cargo_toml_fallback() {
    let root = tmp("tsrs_cargo_fallback");
    let src_tauri = setup_workspace(&root);
    // No .cargo/config.toml, but Cargo.toml mentions ts-rs
    fs::write(
        src_tauri.join("Cargo.toml"),
        r#"
[package]
name = "test"

[dependencies]
ts-rs = "10"
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let tsrs = gens.iter().find(|g| g.kind == GeneratorKind::TsRs);
    assert!(tsrs.is_some(), "should find ts-rs from Cargo.toml dep");
    let g = tsrs.unwrap();
    assert!(g.is_directory);
    // Default is src-tauri/bindings
    assert!(
        g.output_path.ends_with("src-tauri/bindings"),
        "default path should be src-tauri/bindings, got: {}",
        g.output_path.display()
    );
}

#[test]
fn test_ts_rs_not_found_without_dep_or_config() {
    let root = tmp("tsrs_nothing");
    let src_tauri = setup_workspace(&root);
    fs::write(
        src_tauri.join("Cargo.toml"),
        "[package]\nname = \"test\"\n[dependencies]\nserde = \"1\"\n",
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    assert!(
        gens.iter().all(|g| g.kind != GeneratorKind::TsRs),
        "should not find ts-rs without config or dep"
    );
}

// ─── Typegen ─────────────────────────────────────────────────────────────

#[test]
fn test_typegen_camel_case_output_path() {
    let root = tmp("typegen_camel");
    let src_tauri = setup_workspace(&root);
    fs::write(
        src_tauri.join("tauri.conf.json"),
        r#"{
            "identifier": "com.test",
            "plugins": {
                "typegen": {
                    "projectPath": ".",
                    "outputPath": "../src/types/typegen"
                }
            }
        }"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let tg = gens.iter().find(|g| g.kind == GeneratorKind::Typegen);
    assert!(tg.is_some(), "should find typegen from camelCase config");
    let g = tg.unwrap();
    assert!(g.is_directory);
    assert!(
        g.output_path.ends_with("src/types/typegen"),
        "path was: {}",
        g.output_path.display()
    );
}

#[test]
fn test_typegen_default_output_path() {
    let root = tmp("typegen_default");
    let src_tauri = setup_workspace(&root);
    // plugins.typegen exists but no outputPath → default ../src/generated
    fs::write(
        src_tauri.join("tauri.conf.json"),
        r#"{
            "identifier": "com.test",
            "plugins": {
                "typegen": {
                    "projectPath": "."
                }
            }
        }"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let tg = gens.iter().find(|g| g.kind == GeneratorKind::Typegen);
    assert!(tg.is_some(), "should find typegen with default path");
    assert!(
        tg.unwrap().output_path.ends_with("src/generated"),
        "default should be ../src/generated resolved from src-tauri, got: {}",
        tg.unwrap().output_path.display()
    );
}

#[test]
fn test_typegen_not_found_without_plugin_section() {
    let root = tmp("typegen_no_plugin");
    let _src_tauri = setup_workspace(&root);
    // tauri.conf.json has no plugins.typegen

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    assert!(
        gens.iter().all(|g| g.kind != GeneratorKind::Typegen),
        "should not find typegen without plugins.typegen"
    );
}

// ─── Standalone specta-typescript ─────────────────────────────────────────

#[test]
fn test_specta_typescript_export_to() {
    let root = tmp("specta_ts_standalone");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.rs"),
        r#"
use specta_typescript::Typescript;
fn main() {
    Typescript::default()
        .export_to("../src/bindings.ts", &types)
        .expect("Failed to export");
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    let st = gens
        .iter()
        .find(|g| g.kind == GeneratorKind::TsRs && !g.is_directory);
    assert!(st.is_some(), "should find specta-typescript as TsRs (file)");
    let g = st.unwrap();
    assert!(
        g.output_path.ends_with("src/bindings.ts"),
        "path was: {}",
        g.output_path.display()
    );
}

#[test]
fn test_specta_typescript_not_found_without_import() {
    let root = tmp("specta_ts_no_import");
    let src_tauri = setup_workspace(&root);
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.rs"),
        r#"
fn main() {
    something.export_to("../src/bindings.ts", &types).unwrap();
}
"#,
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    // Should not match because content doesn't contain "specta_typescript"
    assert!(
        !gens
            .iter()
            .any(|g| g.kind == GeneratorKind::TsRs && !g.is_directory),
        "should not find specta-typescript without specta_typescript in content"
    );
}

// ─── Combined ────────────────────────────────────────────────────────────

#[test]
fn test_all_three_generators_discovered() {
    let root = tmp("all_three");
    let src_tauri = setup_workspace(&root);

    // Specta: .rs file with .export()
    let src = src_tauri.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
use tauri_specta::{Builder};
fn run() {
    builder.export(Typescript::default(), "../src/specta_bindings.ts").unwrap();
}
"#,
    )
    .unwrap();

    // Typegen: in tauri.conf.json
    fs::write(
        src_tauri.join("tauri.conf.json"),
        r#"{
            "identifier": "com.test",
            "plugins": {
                "typegen": {
                    "outputPath": "../src/types/typegen"
                }
            }
        }"#,
    )
    .unwrap();

    // ts-rs: .cargo/config.toml
    let cargo_dir = root.join(".cargo");
    fs::create_dir_all(&cargo_dir).unwrap();
    fs::write(
        cargo_dir.join("config.toml"),
        "[env]\nTS_RS_EXPORT_DIR = \"../src/types/ts-rs\"\n",
    )
    .unwrap();

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    assert_eq!(gens.len(), 3, "should find all 3 generators");
    assert!(gens.iter().any(|g| g.kind == GeneratorKind::Specta));
    assert!(gens.iter().any(|g| g.kind == GeneratorKind::TsRs));
    assert!(gens.iter().any(|g| g.kind == GeneratorKind::Typegen));
}

#[test]
fn test_no_generators_without_tauri_config() {
    let root = tmp("no_tauri");
    fs::create_dir_all(&root).unwrap();
    // No tauri.conf.json at all

    let gens = discover_generators(&root);
    let _ = fs::remove_dir_all(&root);

    assert!(gens.is_empty(), "no generators without tauri.conf.json");
}
