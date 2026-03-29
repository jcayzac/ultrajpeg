use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if let Err(error) = real_main() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn Error>> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should live under the workspace root")
        .to_path_buf();

    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("release") => {
            if let Some(unexpected) = args.next() {
                return Err(format!("unexpected argument: {unexpected}").into());
            }
            release(&repo_root)
        }
        _ => Err("usage:\n  cargo run -p xtask -- release".into()),
    }
}

fn release(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    ensure_clean_workdir(repo_root)?;

    let version = read_package_version(&repo_root.join("Cargo.toml"))?;
    let tag = format!("v{version}");

    run(Command::new("git")
        .current_dir(repo_root)
        .arg("tag")
        .arg(&tag))?;
    let push_status = Command::new("git")
        .current_dir(repo_root)
        .arg("push")
        .arg("origin")
        .arg(&tag)
        .status()?;
    if !push_status.success() {
        return Err(format!(
            "failed to push {tag} to origin; the local tag was created successfully. Push it manually with: git push origin {tag}"
        )
        .into());
    }

    Ok(())
}

fn read_package_version(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut in_package = false;

    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }

        if !in_package {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }

        let version = value.trim().trim_matches('"');
        if version.is_empty() {
            return Err(format!("package version is empty in {}", path.display()).into());
        }
        return Ok(version.to_owned());
    }

    Err(format!("could not find package.version in {}", path.display()).into())
}

fn ensure_clean_workdir(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    let status = command_output(
        Command::new("git")
            .current_dir(repo_root)
            .arg("status")
            .arg("--short")
            .arg("--untracked-files=normal"),
    )?;

    if !status.trim().is_empty() {
        return Err("refusing to release from a dirty working tree".into());
    }

    Ok(())
}

fn run(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command {:?} failed with status {status}", command).into());
    }
    Ok(())
}

fn command_output(command: &mut Command) -> Result<String, Box<dyn Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!("command {:?} failed with status {}", command, output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
