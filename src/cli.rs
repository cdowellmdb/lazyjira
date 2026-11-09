//! Command-line flags and dev mode: `--help`, `--version`, and the `--dev` rebuilds from source.

use anyhow::Result;
use std::process::Command;

fn dev_manifest_path(
    working_dir: &std::path::Path,
    compiled_dir: &std::path::Path,
) -> std::path::PathBuf {
    working_dir
        .ancestors()
        .map(|dir| dir.join("Cargo.toml"))
        .find(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
                .is_some_and(|manifest| {
                    manifest
                        .get("package")
                        .and_then(|package| package.get("name"))
                        .and_then(toml::Value::as_str)
                        == Some("lazyjira")
                })
        })
        .unwrap_or_else(|| compiled_dir.join("Cargo.toml"))
}

pub fn handle_flags() -> Result<()> {
    let mut force_rebuild = false;
    let mut release = false;
    let mut show_help = false;
    let mut passthrough_args = Vec::new();

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--dev" | "--rebuild" => force_rebuild = true,
            "--dev-release" => {
                force_rebuild = true;
                release = true;
            }
            "--help" | "-h" => show_help = true,
            "--version" | "-V" => {
                println!("lazyjira {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            _ => passthrough_args.push(arg),
        }
    }

    if show_help {
        println!("lazyjira");
        println!("  --dev, --rebuild   Build from source and run (debug)");
        println!("  --dev-release      Build from source and run (release)");
        println!("  -V, --version      Show the version");
        println!("  -h, --help         Show this help");
        std::process::exit(0);
    }

    if !force_rebuild {
        return Ok(());
    }

    let manifest = dev_manifest_path(
        &std::env::current_dir()?,
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
    );
    anyhow::ensure!(
        manifest.is_file(),
        "No lazyjira source checkout found. Run --dev from a lazyjira checkout."
    );
    println!("Building lazyjira from {}", manifest.display());
    let mut cmd = Command::new("cargo");
    cmd.current_dir(manifest.parent().unwrap());
    cmd.arg("run");

    if release {
        cmd.arg("--release");
    }

    cmd.arg("--manifest-path").arg(manifest);

    if !passthrough_args.is_empty() {
        cmd.arg("--");
        cmd.args(passthrough_args);
    }

    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_mode_prefers_the_active_checkout_over_the_installed_git_source() {
        let root = std::env::temp_dir().join(format!("lazyjira-dev-path-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let checkout = root.join("checkout");
        let nested = checkout.join("src/widgets");
        let compiled = root.join("cargo/git/old-release");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&compiled).unwrap();
        let local_manifest = checkout.join("Cargo.toml");
        let compiled_manifest = compiled.join("Cargo.toml");
        std::fs::write(&compiled_manifest, "[package]\nname = 'lazyjira'\n").unwrap();
        std::fs::write(&local_manifest, "[package]\nname = 'different-project'\n").unwrap();
        assert_eq!(dev_manifest_path(&nested, &compiled), compiled_manifest);
        std::fs::write(&local_manifest, "[package]\nname = 'lazyjira'\n").unwrap();
        assert_eq!(dev_manifest_path(&checkout, &compiled), local_manifest);
        assert_eq!(dev_manifest_path(&nested, &compiled), local_manifest);
        std::fs::write(&local_manifest, "invalid TOML").unwrap();
        assert_eq!(dev_manifest_path(&nested, &compiled), compiled_manifest);
        std::fs::remove_dir_all(root).unwrap();
    }
}
