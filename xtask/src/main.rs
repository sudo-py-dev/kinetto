//! Workspace checks. `cargo xtask layers` fails if a crate depends on a crate
//! in a higher layer, or if UI libraries leak below the UI layer.

use serde_json::Value;
use std::process::{Command, ExitCode};

/// Layer of each workspace crate. Dependencies may only point to a lower or
/// equal layer.
const LAYERS: &[(&str, u8)] = &[
    ("kn-time", 0),
    ("kn-geom", 0),
    ("kn-model", 1),
    ("kn-text", 1),
    ("kn-media", 1),
    ("kn-layout", 2),
    ("kn-render", 3),
    ("kn-export", 3),
    ("kn-engine", 4),
    ("kn-ui", 5),
    ("kinetto", 6),
    ("kinetto-cli", 6),
];

/// External crates allowed only from this layer upward.
const UI_ONLY: &[&str] = &["eframe", "egui", "egui-wgpu", "winit", "rfd", "cpal", "muda"];
const UI_LAYER: u8 = 5;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("layers") => layers(),
        _ => {
            eprintln!("usage: cargo xtask layers");
            ExitCode::FAILURE
        }
    }
}

fn layer_of(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

fn layers() -> ExitCode {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("failed to run cargo metadata");
    let meta: Value = serde_json::from_slice(&output.stdout).expect("cargo metadata returned invalid JSON");

    let mut errors = Vec::new();
    for package in meta["packages"].as_array().into_iter().flatten() {
        let name = package["name"].as_str().unwrap_or_default();
        if name == "xtask" {
            continue;
        }
        let Some(layer) = layer_of(name) else {
            errors.push(format!("{name}: not assigned a layer in xtask LAYERS"));
            continue;
        };
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            if dep["kind"].as_str() == Some("dev") {
                continue;
            }
            let dep_name = dep["name"].as_str().unwrap_or_default();
            if let Some(dep_layer) = layer_of(dep_name) {
                if dep_layer > layer {
                    errors.push(format!("{name} (L{layer}) depends on {dep_name} (L{dep_layer}), which is higher"));
                }
            } else if UI_ONLY.contains(&dep_name) && layer < UI_LAYER {
                errors.push(format!("{name} (L{layer}) uses UI crate {dep_name}; only L{UI_LAYER}+ may"));
            }
        }
    }

    if errors.is_empty() {
        println!("layers ok");
        ExitCode::SUCCESS
    } else {
        errors.iter().for_each(|e| eprintln!("layer violation: {e}"));
        ExitCode::FAILURE
    }
}
