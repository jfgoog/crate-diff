use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::process::{Command, Output};
use std::str::from_utf8;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
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
    Deps { v1: String, v2: String },
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
        Cmd::Deps { v1, v2 } => {
            let index = crates_index::GitIndex::new_cargo_default()?;
            let krate = index
                .crate_(&args.crate_name)
                .ok_or(anyhow!("Couldn't find crate name {}", args.crate_name))?;
            let v1 = krate
                .versions()
                .iter()
                .find(|v| v.version() == v1)
                .cloned()
                .ok_or(anyhow!(
                    "Couldn't find version {} for crate {}",
                    v1,
                    args.crate_name
                ))?;
            let v2 = krate
                .versions()
                .iter()
                .find(|v| v.version() == v2)
                .cloned()
                .ok_or(anyhow!(
                    "Couldn't find version {} for crate {}",
                    v2,
                    args.crate_name
                ))?;
            let v1_map = v1
                .dependencies()
                .iter()
                .map(|d| (d.name(), d))
                .collect::<BTreeMap<_, _>>();
            let v2_map = v2
                .dependencies()
                .iter()
                .map(|d| (d.name(), d))
                .collect::<BTreeMap<_, _>>();
            let v1_deps = v1
                .dependencies()
                .iter()
                .map(|d| d.name())
                .collect::<BTreeSet<_>>();
            let v2_deps = v2
                .dependencies()
                .iter()
                .map(|d| d.name())
                .collect::<BTreeSet<_>>();
            let added = v2_deps.difference(&v1_deps).collect::<HashSet<_>>();
            let removed = v1_deps.difference(&v2_deps).collect::<HashSet<_>>();

            let dir = tempfile::tempdir()?;
            for dep in v1_deps.union(&v2_deps) {
                if added.contains(dep) {
                    println!(
                        "{}",
                        format!("+{:#?}", v2_map.get(dep).unwrap())
                            .lines()
                            .collect::<Vec<_>>()
                            .join("\n+")
                    );
                } else if removed.contains(dep) {
                    println!(
                        "{}",
                        format!("-{:#?}", v1_map.get(dep).unwrap())
                            .lines()
                            .collect::<Vec<_>>()
                            .join("\n-")
                    );
                } else {
                    std::fs::write(
                        dir.as_ref()
                            .join(format!("{}-{}", &args.crate_name, v1.version())),
                        format!("{:#?}\n", v1_map.get(dep).unwrap()),
                    )?;
                    std::fs::write(
                        dir.as_ref()
                            .join(format!("{}-{}", &args.crate_name, v2.version())),
                        format!("{:#?}\n", v2_map.get(dep).unwrap()),
                    )?;
                    let diff = Command::new("diff")
                        .args([
                            "-w",
                            // "--color=always",
                            "--unified=1000",
                        ])
                        .args([
                            format!("{}-{}", args.crate_name, v1.version()),
                            format!("{}-{}", args.crate_name, v2.version()),
                        ])
                        .current_dir(&dir)
                        .output()?;
                    if !diff.status.success() {
                        println!(
                            "{}",
                            from_utf8(&diff.stdout)?
                                .lines()
                                .skip(3)
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                    }
                }
            }
            dir.close()?
        }
    }
    Ok(())
}
