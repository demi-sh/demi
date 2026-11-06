//! `xtask sdk-release`: assembles the npm packages a program runs Demi's
//! agents with, from the executables `xtask native build` wrote and the
//! workspace's TypeScript builds (`bun run build`), one directory for each
//! package, as `npm pack` takes it:
//!
//! ```text
//! <output>/
//!   sdk/                     @demicodes/sdk
//!     package.json           at the build version, with each platform's
//!                            package an optional dependency
//!     dist/                  its build
//!     release/commands/      every command package's descriptor
//!   sdk-darwin-arm64/        @demicodes/sdk-darwin-arm64, one for each target
//!     package.json           with its `os` and `cpu`
//!     bin/demi-core          the target's process and runner
//!     bin/demi-runner
//!     release/files/         the target's command programs, compressed
//!   web-components/          @demicodes/web-components
//!   conversation-client/     each package of the workspace the SDK or the
//!   protocol/                web components depend on, which they are
//!   ...                      published beside
//! ```
//!
//! A package manager installs only the platform package whose `os` and `cpu`
//! are the machine's, as esbuild's are installed. The SDK finds its
//! executables and release in this layout (`packages/sdk/src/binaries.ts`).
//!
//! Every package is at the build version, and so is each dependency of one
//! in the workspace. A workspace package's manifest leaves out what only the
//! workspace uses: its `development` export, which names its sources, its
//! scripts and its development dependencies.

use std::path::{Path, PathBuf};

use demi_runner_protocol::release::{FILES, compressed_file};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::native::{self, Caches, Executable, Spec, Versioning};

/// The SDK's main package's name, and the prefix of each platform package's.
const SDK: &str = "@demicodes/sdk";

/// The scope of the workspace's packages, each in the directory of its
/// name under `packages/`.
const SCOPE: &str = "@demicodes/";

/// The workspace's packages a program installs, by directory, besides those
/// they depend on.
const PROGRAM_PACKAGES: [&str; 2] = ["sdk", "web-components"];

#[derive(clap::Args)]
pub struct Options {
    /// The directory to assemble the packages in, which does not exist yet.
    #[arg(long, value_name = "DIRECTORY")]
    output: PathBuf,
    /// A target of the platform packages; repeat for several [default: all
    /// six].
    #[arg(long = "target", value_name = "TRIPLE", value_parser = native::target)]
    targets: Vec<&'static str>,
    /// Names the command packages with the build version itself; only the
    /// build workflow passes it.
    #[arg(long)]
    publish: bool,
    /// The Cargo target directory the build wrote [default:
    /// .cache/native-target in the repository].
    #[arg(long, value_name = "DIRECTORY")]
    artifacts: Option<PathBuf>,
    #[command(flatten)]
    caches: Caches,
}

/// Why assembling stopped.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{} exists: the SDK's packages are assembled once", .0.display())]
    Exists(PathBuf),
    #[error("{} has no parent directory to assemble the packages beside", .0.display())]
    NoParent(PathBuf),
    #[error("{} is not built: `bun run build` builds the workspace's packages", .0.display())]
    NotBuilt(PathBuf),
    #[error("{}: {}", .0.display(), .1)]
    Manifest(PathBuf, String),
    #[error(transparent)]
    Stage(#[from] crate::server_release::Error),
    #[error(transparent)]
    Native(#[from] native::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn run(options: Options) -> Result<(), Error> {
    let assembled = crate::interruptible(|cancel| async move { assemble(&options, &cancel).await })??;
    println!("SDK packages: {}", assembled.display());
    Ok(())
}

/// Assembles the packages `options` names and returns their directory.
async fn assemble(options: &Options, cancel: &CancellationToken) -> Result<PathBuf, Error> {
    let output = std::path::absolute(&options.output)?;
    if tokio::fs::try_exists(&output).await? {
        return Err(Error::Exists(output));
    }
    let repository = crate::repository();
    let workspace = workspace_packages(&repository).await?;
    for package in &workspace {
        if !tokio::fs::try_exists(package.source.join("dist")).await? {
            return Err(Error::NotBuilt(package.source.join("dist")));
        }
    }
    let parent = output
        .parent()
        .ok_or_else(|| Error::NoParent(output.clone()))?
        .to_owned();
    tokio::fs::create_dir_all(&parent).await?;
    let name = output
        .file_name()
        .ok_or_else(|| Error::NoParent(output.clone()))?
        .to_string_lossy()
        .into_owned();
    // The stages are removed however assembling ends, unless the packages'
    // becomes the output.
    let stage = crate::server_release::staged(&parent, format!(".{name}-stage-")).await?;
    let packaged = crate::server_release::staged(&parent, format!(".{name}-packages-")).await?;
    let artifacts = native::artifacts(options.artifacts.as_deref())?;
    let versioning = if options.publish {
        Versioning::Published
    } else {
        Versioning::Development
    };
    let version = demi_shared_artifacts::BUILD_VERSION;
    let targets = native::targets(&[Executable::Runner, Executable::Core], &options.targets)?;

    // The command packages: their descriptors in the main package, their
    // programs gathered here, then handed to each target's package.
    let main = stage.path().join("sdk");
    let release = main.join("release");
    let gathered = packaged.path().join(FILES);
    for executable in Executable::COMMANDS {
        let targets = native::targets(&[executable], &targets)?;
        let output = packaged.path().join(executable.name());
        let spec = Spec {
            executable,
            targets: &targets,
            artifacts: &artifacts,
            output: &output,
            caches: &options.caches,
            versioning,
            version,
        };
        println!("{}", native::package(&spec, cancel).await?);
        native::split(&output, executable, &release, &gathered, cancel).await?;
    }

    let mut platforms = Map::new();
    for &target in &targets {
        let (os, cpu) = platform(target);
        let package = format!("{SDK}-{os}-{cpu}");
        let directory = stage.path().join(format!("sdk-{os}-{cpu}"));
        let bin = directory.join("bin");
        tokio::fs::create_dir_all(&bin).await?;
        for executable in [Executable::Core, Executable::Runner] {
            let built = native::built(&artifacts, executable, target);
            if !tokio::fs::try_exists(&built).await? {
                return Err(native::Error::NotBuilt {
                    executable,
                    target,
                    path: built,
                }
                .into());
            }
            // `copy` keeps the executable's permissions.
            tokio::fs::copy(&built, bin.join(executable.file_name(target))).await?;
        }
        let files = directory.join("release").join(FILES);
        tokio::fs::create_dir_all(&files).await?;
        for executable in Executable::COMMANDS {
            if !executable.targets().contains(&target) {
                continue;
            }
            let file = compressed_file(executable.name(), target);
            tokio::fs::rename(gathered.join(&file), files.join(&file)).await?;
        }
        let manifest = json!({
            "name": package,
            "version": version,
            "description": format!("The executables and command programs of {SDK} for {}.", described(target)),
            "license": "Apache-2.0",
            "os": [os],
            "cpu": [cpu],
            "files": ["bin", "release"],
            // Yarn's Plug'n'Play runs no executable from inside a zip.
            "preferUnplugged": true,
            "repository": {
                "type": "git",
                "url": "git+https://github.com/demi-sh/demi.git",
            },
        });
        write_manifest(&directory, &manifest).await?;
        legal(&directory).await?;
        platforms.insert(package, Value::String(version.to_owned()));
    }

    for package in workspace {
        let directory = stage.path().join(&package.directory);
        tokio::fs::create_dir_all(&directory).await?;
        crate::server_release::copy_directory(package.source.join("dist"), directory.join("dist")).await?;
        tokio::fs::copy(package.source.join("README.md"), directory.join("README.md")).await?;
        let mut manifest = published_manifest(package.manifest, version);
        if package.directory == "sdk"
            && let Some(object) = manifest.as_object_mut()
        {
            object.insert("optionalDependencies".to_owned(), Value::Object(platforms.clone()));
        }
        write_manifest(&directory, &manifest).await?;
        legal(&directory).await?;
    }

    if cancel.is_cancelled() {
        return Err(native::Error::Artifact(demi_shared_artifacts::Error::Cancelled).into());
    }
    let staged = stage.keep();
    if let Err(error) = tokio::fs::rename(&staged, &output).await {
        // Nothing else holds the stage: it goes rather than lingering.
        let _ = tokio::fs::remove_dir_all(&staged).await;
        return Err(error.into());
    }
    Ok(output)
}

/// A package of the workspace, as it is in the repository.
struct WorkspacePackage {
    /// Its directory's name under `packages/`, which names its directory in
    /// the output too.
    directory: String,
    source: PathBuf,
    manifest: Value,
}

/// [`PROGRAM_PACKAGES`] and every package of the workspace they depend on.
async fn workspace_packages(repository: &Path) -> Result<Vec<WorkspacePackage>, Error> {
    let mut packages: Vec<WorkspacePackage> = Vec::new();
    let mut waiting: Vec<String> = PROGRAM_PACKAGES.iter().map(|&directory| directory.to_owned()).collect();
    while let Some(directory) = waiting.pop() {
        if packages.iter().any(|package| package.directory == directory) {
            continue;
        }
        let source = repository.join("packages").join(&directory);
        let path = source.join("package.json");
        let manifest: Value = serde_json::from_slice(&tokio::fs::read(&path).await?)
            .map_err(|error| Error::Manifest(path.clone(), error.to_string()))?;
        if manifest.get("name").and_then(Value::as_str) != Some(&format!("{SCOPE}{directory}")) {
            return Err(Error::Manifest(path, format!("not named {SCOPE}{directory}")));
        }
        if let Some(Value::Object(dependencies)) = manifest.get("dependencies") {
            for (name, range) in dependencies {
                if !range.as_str().is_some_and(|range| range.starts_with("workspace:")) {
                    continue;
                }
                let dependency = name
                    .strip_prefix(SCOPE)
                    .ok_or_else(|| Error::Manifest(path.clone(), format!("{name} is in the workspace but not in {SCOPE}")))?;
                waiting.push(dependency.to_owned());
            }
        }
        packages.push(WorkspacePackage {
            directory,
            source,
            manifest,
        });
    }
    Ok(packages)
}

/// A workspace package's `manifest` as its package publishes it: at
/// `version`, with its dependencies in the workspace at `version` too, and
/// without its `development` export, its scripts and its development
/// dependencies.
fn published_manifest(mut manifest: Value, version: &str) -> Value {
    let Some(object) = manifest.as_object_mut() else {
        return manifest;
    };
    object.insert("version".to_owned(), Value::String(version.to_owned()));
    if let Some(Value::Object(dependencies)) = object.get_mut("dependencies") {
        for value in dependencies.values_mut() {
            if value.as_str().is_some_and(|range| range.starts_with("workspace:")) {
                *value = Value::String(version.to_owned());
            }
        }
    }
    if let Some(Value::Object(exports)) = object.get_mut("exports") {
        for entry in exports.values_mut() {
            if let Value::Object(conditions) = entry {
                conditions.shift_remove("development");
            }
        }
    }
    object.shift_remove("scripts");
    object.shift_remove("devDependencies");
    manifest
}

/// npm's `os` and `cpu` of `target`, as Node.js names its platform and
/// architecture.
fn platform(target: &str) -> (&'static str, &'static str) {
    let os = if target.contains("apple") {
        "darwin"
    } else if target.contains("windows") {
        "win32"
    } else {
        "linux"
    };
    let cpu = if target.starts_with("aarch64") { "arm64" } else { "x64" };
    (os, cpu)
}

/// `target` in words, as a platform package's description says it.
fn described(target: &str) -> &'static str {
    match platform(target) {
        ("darwin", "arm64") => "macOS on ARM64",
        ("darwin", _) => "macOS on x86-64",
        ("win32", "arm64") => "Windows on ARM64",
        ("win32", _) => "Windows on x86-64",
        (_, "arm64") => "Linux on ARM64",
        _ => "Linux on x86-64",
    }
}

async fn write_manifest(directory: &Path, manifest: &Value) -> Result<(), Error> {
    let mut text = serde_json::to_string_pretty(manifest).map_err(|error| Error::Manifest(directory.join("package.json"), error.to_string()))?;
    text.push('\n');
    tokio::fs::write(directory.join("package.json"), text).await?;
    Ok(())
}

/// The project's license and notice, which each package carries.
async fn legal(directory: &Path) -> Result<(), Error> {
    for (name, text) in crate::server_release::LEGAL {
        tokio::fs::write(directory.join(name), text).await?;
    }
    Ok(())
}
