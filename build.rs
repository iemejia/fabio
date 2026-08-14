/// Build script that auto-registers context data files.
/// Scans `data/best_practices/` and `data/workflows/` at compile time,
/// emits Rust source to `OUT_DIR` — no generated files in the source tree.
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    generate_entries(
        "src/commands/context/data/best_practices",
        &format!("{out_dir}/best_practices.rs"),
        "BEST_PRACTICES",
    );
    generate_entries(
        "src/commands/context/data/workflows",
        &format!("{out_dir}/workflows.rs"),
        "WORKFLOWS",
    );
    generate_entries(
        "src/commands/context/data/personas",
        &format!("{out_dir}/personas.rs"),
        "PERSONAS",
    );
    generate_entries(
        "src/commands/context/data/disambiguations",
        &format!("{out_dir}/disambiguations.rs"),
        "DISAMBIGUATIONS",
    );
    generate_entries(
        "src/commands/context/data/skills",
        &format!("{out_dir}/skills.rs"),
        "SKILLS",
    );
    generate_entries(
        "src/commands/context/data/blueprints",
        &format!("{out_dir}/blueprints.rs"),
        "BLUEPRINTS",
    );

    println!("cargo::rerun-if-changed=src/commands/context/data/best_practices");
    println!("cargo::rerun-if-changed=src/commands/context/data/workflows");
    println!("cargo::rerun-if-changed=src/commands/context/data/personas");
    println!("cargo::rerun-if-changed=src/commands/context/data/disambiguations");
    println!("cargo::rerun-if-changed=src/commands/context/data/skills");
    println!("cargo::rerun-if-changed=src/commands/context/data/blueprints");
}

fn generate_entries(data_dir: &str, output_file: &str, const_name: &str) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = Path::new(&manifest_dir).join(data_dir);
    let mut entries: Vec<(String, String)> = Vec::new();

    if let Ok(read_dir) = fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let stem = path.file_stem().unwrap().to_string_lossy().to_string();
                let topic_name = stem.replace('_', "-");
                // Absolute path so include_str! works from OUT_DIR
                let abs_path = path.to_string_lossy().replace('\\', "/");
                entries.push((topic_name, abs_path));
            }
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut file = fs::File::create(output_file).unwrap();
    writeln!(file, "const {const_name}: &[(&str, &str)] = &[").unwrap();
    for (name, path) in &entries {
        writeln!(file, "    (\"{name}\", include_str!(\"{path}\")),").unwrap();
    }
    writeln!(file, "];").unwrap();
}
