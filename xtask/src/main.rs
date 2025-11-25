use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::blocking;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use tempfile::NamedTempFile;
use zip::ZipArchive;

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MINECRAFT_DATA_ZIP_URL: &str = "https://codeload.github.com/PrismarineJS/minecraft-data/zip";

#[derive(Parser)]
#[command(
    about = "Automation helpers for the Brine workspace",
    version,
    author = "Brine Devs"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download Minecraft assets (assets/, data/, pack.mcmeta) for a version.
    FetchAssets {
        /// Minecraft version identifier (e.g., 1.21.4).
        #[arg(long)]
        version: String,
        /// Re-download even if the target directory already exists.
        #[arg(long)]
        force: bool,
    },
    /// Refresh the bundled minecraft-data files from PrismarineJS.
    FetchMinecraftData {
        /// Git reference to download (branch, tag, or commit).
        #[arg(long, default_value = "master")]
        reference: String,
    },
    /// Refresh minecraft-data and download the requested game's assets.
    Setup {
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "master")]
        reference: String,
        #[arg(long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::FetchAssets { version, force } => fetch_assets(&version, force),
        Command::FetchMinecraftData { reference } => fetch_minecraft_data(&reference),
        Command::Setup {
            version,
            reference,
            force,
        } => {
            fetch_minecraft_data(&reference)?;
            fetch_assets(&version, force)
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate lives in workspace root")
        .to_path_buf()
}

fn fetch_assets(version: &str, force: bool) -> Result<()> {
    let root = workspace_root();
    let output_dir = root.join("assets").join(version);

    if output_dir.exists() {
        if force {
            fs::remove_dir_all(&output_dir).with_context(|| {
                format!(
                    "failed to remove existing assets at {}",
                    output_dir.display()
                )
            })?;
        } else {
            println!(
                "Assets for {version} already exist at {}, skipping",
                output_dir.display()
            );
            return Ok(());
        }
    }

    println!("Downloading Minecraft {version} client metadata");
    let manifest: VersionManifest = fetch_json(VERSION_MANIFEST_URL)?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == version)
        .ok_or_else(|| anyhow!("Version {version} not found in the Mojang manifest"))?;

    let details: VersionDetails = fetch_json(&entry.url)?;
    let client_url = details.downloads.client.url;

    println!("Downloading client.jar (this may take a moment)");
    let temp_file = NamedTempFile::new()?;
    download_to_path(&client_url, temp_file.path())
        .with_context(|| format!("failed to download client jar from {client_url}"))?;

    println!("Extracting assets and data to {}", output_dir.display());
    let pack_exists = extract_client_payload(temp_file.path(), &output_dir)?;
    ensure_pack_metadata(
        &output_dir,
        version,
        details.pack_version.as_ref(),
        pack_exists,
    )?;

    println!("Assets for {version} ready at {}", output_dir.display());
    Ok(())
}

fn fetch_minecraft_data(reference: &str) -> Result<()> {
    let root = workspace_root();
    let base = root.join("third_party").join("minecraft-data-rs");
    if !base.exists() {
        bail!(
            "{} is missing; ensure the repository is checked out",
            base.display()
        );
    }
    let target = base.join("minecraft-data");
    if target.exists() {
        fs::remove_dir_all(&target)
            .with_context(|| format!("failed to clear {}", target.display()))?;
    }
    fs::create_dir_all(&target)?;

    let url = format!("{MINECRAFT_DATA_ZIP_URL}/{}", reference);
    println!("Downloading minecraft-data ({reference})");
    let temp_file = NamedTempFile::new()?;
    download_to_path(&url, temp_file.path())
        .with_context(|| format!("failed to download minecraft-data archive from {url}"))?;

    println!("Extracting minecraft-data into {}", target.display());
    extract_repo_archive(temp_file.path(), &target)?;
    println!("minecraft-data refreshed from {reference}");
    Ok(())
}

fn download_to_path(url: &str, destination: &Path) -> Result<()> {
    let mut response = blocking::get(url).with_context(|| format!("failed to download {url}"))?;

    let mut writer = File::create(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    io::copy(&mut response, &mut writer)?;
    Ok(())
}

fn fetch_json<T: DeserializeOwned>(url: &str) -> Result<T> {
    blocking::get(url)
        .with_context(|| format!("failed to download {url}"))?
        .json()
        .map_err(|err| anyhow!("failed to parse JSON from {url}: {err}"))
}

fn extract_client_payload(jar_path: &Path, destination: &Path) -> Result<bool> {
    fs::create_dir_all(destination)?;

    let file = File::open(jar_path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut pack_found = false;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(rel_path) = entry.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let rel_str = rel_path.to_string_lossy();
        let include = rel_str.starts_with("assets/")
            || rel_str.starts_with("data/")
            || rel_str == "pack.mcmeta";
        if !include {
            continue;
        }

        let out_path = destination.join(&*rel_path);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut outfile = File::create(&out_path)?;
        io::copy(&mut entry, &mut outfile)?;

        if rel_str == "pack.mcmeta" {
            pack_found = true;
        }
    }

    Ok(pack_found)
}

fn extract_repo_archive(zip_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(path) = entry.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };

        // Skip the top-level directory that GitHub archives wrap files in.
        let mut components = path.components();
        if components.next().is_none() {
            continue;
        }
        let relative: PathBuf = components.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }

        let out_path = destination.join(relative);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut outfile = File::create(&out_path)?;
        io::copy(&mut entry, &mut outfile)?;
    }

    Ok(())
}

#[derive(Deserialize)]
struct VersionManifest {
    versions: Vec<VersionEntry>,
}

#[derive(Deserialize)]
struct VersionEntry {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct VersionDetails {
    downloads: VersionDownloads,
    #[serde(default)]
    pack_version: Option<PackVersion>,
}

#[derive(Deserialize)]
struct VersionDownloads {
    client: VersionFile,
}

#[derive(Deserialize)]
struct VersionFile {
    url: String,
}

#[derive(Deserialize)]
struct PackVersion {
    resource: u32,
    #[allow(unused)]
    data: u32,
}

fn ensure_pack_metadata(
    destination: &Path,
    version: &str,
    pack_version: Option<&PackVersion>,
    already_present: bool,
) -> Result<()> {
    let pack_path = destination.join("pack.mcmeta");
    if already_present && pack_path.exists() {
        return Ok(());
    }

    let pack_format = pack_version.map(|p| p.resource).unwrap_or(1);
    let contents = json!({
        "pack": {
            "pack_format": pack_format,
            "description": format!("Minecraft {version} assets")
        }
    });

    fs::write(&pack_path, serde_json::to_string_pretty(&contents)?)?;
    Ok(())
}
