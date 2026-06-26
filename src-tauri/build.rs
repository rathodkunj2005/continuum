use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

const HERMES_SKIP_DIRS: &[&str] = &[
    ".git",
    ".github",
    ".plans",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    "node_modules",
];

const HERMES_SKIP_FILES: &[&str] = &[".DS_Store"];

/// Build-time secret env vars consumed by `option_env!` in `src/main.rs`.
/// Declaring them here makes Cargo recompile when the release pipeline changes
/// their values, so the baked-in secrets never go stale across CI builds.
const BAKED_SECRET_ENV_VARS: &[&str] = &[
    "CONTINUUM_BAKED_SUPABASE_URL",
    "CONTINUUM_BAKED_SUPABASE_ANON_KEY",
    "CONTINUUM_BAKED_SUPABASE_FUNCTIONS_URL",
    "CONTINUUM_BAKED_AGENT_SYNC_SECRET",
    "CONTINUUM_BAKED_ANTHROPIC_API_KEY",
];

fn main() {
    for var in BAKED_SECRET_ENV_VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }
    if let Err(err) = stage_vendored_hermes_bundle() {
        panic!("failed to stage vendored Hermes bundle: {err}");
    }
    tauri_build::build();
}

fn stage_vendored_hermes_bundle() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let source_dir = manifest_dir
        .parent()
        .unwrap_or(manifest_dir.as_path())
        .join("hermes-agent");
    let staging_dir = manifest_dir.join("target").join("hermes-agent-bundle");

    println!("cargo:rerun-if-changed={}", source_dir.display());

    reset_dir(&staging_dir)?;

    if source_dir.exists() {
        copy_dir_filtered(&source_dir, &staging_dir)?;
    }

    Ok(())
}

fn reset_dir(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)
}

fn should_skip(name: &str, is_dir: bool) -> bool {
    if is_dir {
        HERMES_SKIP_DIRS.contains(&name)
    } else {
        HERMES_SKIP_FILES.contains(&name)
    }
}

fn copy_dir_filtered(source: &Path, destination: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_skip(&file_name, file_type.is_dir()) {
            continue;
        }

        let source_path = entry.path();
        let destination_path = destination.join(file_name.as_ref());

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_dir_filtered(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
            let permissions = fs::metadata(&source_path)?.permissions();
            fs::set_permissions(&destination_path, permissions)?;
        }
    }

    Ok(())
}
