use std::path::Path;
use std::process::{Command, Output};
use std::str::from_utf8;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use crates_index;
use reqwest;
use tempfile;

#[derive(Parser)]
struct Cli {
    crate_name: String,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Versions,
    Diff { v1: String, v2: String },
}

fn success_or_error(cmd: &mut Command) -> Result<Output> {
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to run {:?}.\nstdout:\n{}\nstderr:\n{}",
            cmd,
            from_utf8(&output.stdout)?,
            from_utf8(&output.stderr)?
        ));
    }
    Ok(output)
}

fn fetch_crate(name: &str, version: &str, dir: &impl AsRef<Path>) -> Result<()> {
    let path = dir.as_ref().join(format!("{}-{}.tar.gz", name, version));
    std::fs::write(
        &path,
        reqwest::blocking::get(format!(
            "https://crates.io/api/v1/crates/{}/{}/download",
            name, version
        ))?
        .bytes()?,
    )?;
    success_or_error(Command::new("tar").arg("xf").arg(&path).current_dir(&dir))?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    match args.command {
        Cmd::Versions => {
            let index = crates_index::GitIndex::new_cargo_default()?;
            let krate = index
                .crate_(&args.crate_name)
                .ok_or(anyhow!("Couldn't find crate name {}", args.crate_name))?;
            for v in krate.versions().iter().filter(|v| !v.is_yanked()) {
                println!("{}", v.version())
            }
        }
        Cmd::Diff { v1, v2 } => {
            let dir = tempfile::tempdir()?;
            fetch_crate(&args.crate_name, &v1, &dir.path())?;
            fetch_crate(&args.crate_name, &v2, &dir.path())?;

            let diff = Command::new("diff")
                .args([
                    "-urw",
                    "--color=always",
                    "-x",
                    "ci.yml",
                    "-x",
                    ".cargo_vcs_info.json",
                ])
                .args([
                    format!("{}-{}", args.crate_name, v1),
                    format!("{}-{}", args.crate_name, v2),
                ])
                .current_dir(&dir)
                .output()?;
            println!("{}", from_utf8(&diff.stdout)?);

            dir.close()?
        }
    }
    Ok(())
}
