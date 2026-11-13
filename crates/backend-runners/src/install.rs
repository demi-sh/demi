//! The runner installers (`crates/xtask/src/native/package.rs`): a shell script
//! for macOS and Linux and a PowerShell script for Windows, each naming the
//! backend and one runner release.
//!
//! A script does these steps:
//!
//! 1. It downloads its platform's runner from the backend and checks its
//!    SHA-256.
//! 2. It installs the runner under an installation of its own per backend.
//! 3. It drains an older runner of that installation before it upgrades.
//! 4. It starts the runner, which asks to be paired.
//! 5. It shows the runner's pairing codes until the runner is paired, and the
//!    device's name and the removal command once it is.
//!
//! A script holds no credential: pairing grants device access.
//!
//! The scripts leave the runner's departure from its terminal to the runner
//! (`crates/runner/src/console.rs`) and show only the lines it writes to its
//! log, which `run start` reads the same way (`crates/runner/src/start.rs`), so
//! they never compose them.
//!
//! They never turn on the shell's job control (`set -m`): with `curl … | sh` at
//! a terminal, a shell turning it off again hands the terminal back to curl's
//! ended process group, and dash fails with exit code 2 after the runner has
//! started.
//!
//! The backend composes the install commands from its public URL, as it
//! composes a device's `startCommand`.

use std::path::Path;

use jush_runner_protocol::console::{HANDED_OVER, NO_AUTOSTART, PAIRED, PAIRING_CODE, REMOVAL};
use jush_runner_protocol::installation::{LAUNCHER_PS1, LAUNCHER_SH, installation_name};
use jush_runner_protocol::release::{ReleaseError, RunnerRelease};
use jush_runner_protocol::wire::RunnerPlatform;
use jush_web_api_protocol::devices::InstallCommands;
use url::Url;

/// A backend URL an installer cannot name: not HTTP or HTTPS, or with a
/// user, a password, a query or a fragment.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0} cannot be an installation's backend URL")]
pub struct InvalidBackendUrl(String);

/// `url` as an installer names it.
pub fn backend_url(url: &Url) -> Result<Url, InvalidBackendUrl> {
    let usable = matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    if usable {
        Ok(url.clone())
    } else {
        Err(InvalidBackendUrl(url.to_string()))
    }
}

/// The commands that install and start the runner of `backend` on each
/// system, which Add Device and `jush device install-command` show. They fetch the
/// installer from `backend`'s origin, which serves it whatever path the URL
/// has. `curl` may use only the URL's scheme, and TLS 1.2 or newer for
/// `https`, so a development backend's plain `http` is reached too.
pub fn install_commands(backend: &Url) -> InstallCommands {
    let installer = |name: &str| {
        backend
            .join(name)
            .expect("an absolute path joins any backend URL")
    };
    let shell = installer("/install.sh");
    let tls = if shell.scheme() == "https" {
        " --tlsv1.2"
    } else {
        ""
    };
    // Always quoted, as a person reads it, though a URL rarely needs it.
    let quoted = format!("'{}'", shell.as_str().replace('\'', r"'\''"));
    let unix = format!("curl --proto '={}'{tls} -sSf {quoted} | sh", shell.scheme());
    InstallCommands {
        linux: unix.clone(),
        macos: unix,
        windows: format!("irm {} | iex", powershell(installer("/install.ps1").as_str())),
    }
}

/// The command that starts the runner of `backend`'s installation again on a
/// device of `platform`, typed in a terminal there: the launcher of the
/// installation, in `installation` as its runner reported it or where the
/// installers put it by default, with `start`, which starts the runner in the
/// background (`crates/runner/src/start.rs`).
///
/// A directory in `home`, the device's home directory, is written from the
/// home: `~` in a POSIX shell, `$env:USERPROFILE` in PowerShell, where `-File`
/// does not expand `~`. On Windows the command runs through PowerShell with a
/// policy that lets the launcher run.
pub fn start_command(
    backend: &Url,
    installation: Option<&str>,
    home: Option<&str>,
    platform: RunnerPlatform,
) -> String {
    let windows = platform == RunnerPlatform::Win32;
    let separator = if windows { '\\' } else { '/' };
    let default = format!(".jush{separator}runner{separator}{}", installation_name(backend));
    let place = match installation {
        None => Place::Home(default),
        Some(installation) => {
            let relative = home
                .and_then(|home| installation.strip_prefix(home.trim_end_matches(separator)))
                .and_then(|rest| rest.strip_prefix(separator));
            match relative {
                Some(relative) => Place::Home(relative.to_owned()),
                None => Place::Absolute(installation.to_owned()),
            }
        }
    };
    if windows {
        let launcher = match place {
            Place::Home(relative) => format!(
                "\"$env:USERPROFILE\\{}\\run.ps1\"",
                powershell_expandable(&relative)
            ),
            Place::Absolute(installation) => powershell(&format!("{installation}\\run.ps1")),
        };
        return format!("powershell -ExecutionPolicy Bypass -File {launcher} start");
    }
    match place {
        Place::Home(relative) => format!("~/{} start", sh(&format!("{relative}/run"))),
        Place::Absolute(installation) => format!("{} start", sh(&format!("{installation}/run"))),
    }
}

/// Where an installation's directory is, as a command names it.
enum Place {
    /// Under the home directory, at this path relative to it.
    Home(String),
    Absolute(String),
}

/// `value` inside a PowerShell string literal that expands variables, with
/// nothing of its own expanded.
fn powershell_expandable(value: &str) -> String {
    value
        .replace('`', "``")
        .replace('"', "`\"")
        .replace('$', "`$")
}

/// A server release's `runners/`, where the runner releases are
/// (`crates/backend-http/src/install.rs`):
///
/// - `manifest.json` names the current release, the one the installers install
///   and every paired device's runner follows.
/// - Each release's directory holds its own manifest, which names each target's
///   executable.
///
/// A backend without them, as in development, serves no installer and checks no
/// runner's release.
#[derive(Debug, Clone, Default)]
pub struct RunnerReleases(Option<std::path::PathBuf>);

impl RunnerReleases {
    pub fn new(root: Option<std::path::PathBuf>) -> Self {
        Self(root)
    }

    /// The directory, when the backend has runner releases.
    pub fn root(&self) -> Option<&Path> {
        self.0.as_deref()
    }

    /// The current runner release; none without runner releases. The
    /// record is read on each use, so a release packaged into a running
    /// backend's `runners/` takes effect at once.
    pub async fn current(&self) -> Result<Option<RunnerRelease>, String> {
        let Some(root) = &self.0 else {
            return Ok(None);
        };
        let path = root.join("manifest.json");
        match read_runner_release(&path, RunnerRelease::decode).await? {
            Some(release) => Ok(Some(release)),
            None => Err(format!("{} is missing", path.display())),
        }
    }
}

/// The runner release record at `path`, a release's or the top-level
/// `manifest.json` of a server release's `runners/`, when there is one,
/// read by `decode` ([`RunnerRelease::decode`] for one this build runs); a
/// record that does not decode is the deployment's fault.
pub async fn read_runner_release(
    path: &Path,
    decode: fn(&[u8]) -> Result<RunnerRelease, ReleaseError>,
) -> Result<Option<RunnerRelease>, String> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    decode(&bytes)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// `value` as one POSIX shell word.
fn sh(value: &str) -> String {
    shlex::try_quote(value)
        .expect("an installer's values hold no NUL byte")
        .into_owned()
}

/// `value` as a PowerShell string literal.
fn powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The shell installer of `release` for `backend`.
pub fn shell_script(backend: &Url, release: &RunnerRelease) -> String {
    let cases: Vec<String> = release
        .targets
        .iter()
        .map(|(target, artifact)| {
            format!(
                "\n  {target})\n    runner_hash={}\n    ;;",
                sh(&artifact.sha256)
            )
        })
        .collect();
    SHELL_SCRIPT
        .replace("@LAUNCHER@", LAUNCHER_SH.trim_end())
        .replace("@BACKEND@", &sh(backend.as_str()))
        .replace("@RELEASE@", &sh(&release.release))
        .replace("@INSTALLATION@", &sh(&installation_name(backend)))
        .replace("@BASE@", &sh(&backend.origin().ascii_serialization()))
        .replace("@PAIRING@", &sh(PAIRING_CODE))
        .replace("@PAIRED@", &sh(PAIRED))
        .replace("@REMOVAL@", &sh(REMOVAL))
        .replace("@NO_AUTOSTART@", &sh(NO_AUTOSTART))
        .replace("@HANDED_OVER@", &sh(HANDED_OVER))
        .replace("@CASES@", &cases.join("\n"))
}

/// The PowerShell installer of `release` for `backend`, for the release's
/// Windows targets.
pub fn powershell_script(backend: &Url, release: &RunnerRelease) -> String {
    let artifacts: Vec<String> = release
        .targets
        .iter()
        .filter(|(target, _)| target.contains("windows"))
        .map(|(target, artifact)| {
            format!(
                "  {} = @{{ Hash = {}; Size = {} }}",
                powershell(target),
                powershell(&artifact.sha256),
                artifact.size
            )
        })
        .collect();
    POWERSHELL_SCRIPT
        .replace("@LAUNCHER@", LAUNCHER_PS1.trim_end())
        .replace("@BACKEND@", &powershell(backend.as_str()))
        .replace(
            "@BASE@",
            &powershell(&backend.origin().ascii_serialization()),
        )
        .replace("@RELEASE@", &powershell(&release.release))
        .replace("@INSTALLATION@", &powershell(&installation_name(backend)))
        .replace("@PAIRING@", &powershell(PAIRING_CODE))
        .replace("@PAIRED@", &powershell(PAIRED))
        .replace("@REMOVAL@", &powershell(REMOVAL))
        .replace("@NO_AUTOSTART@", &powershell(NO_AUTOSTART))
        .replace("@HANDED_OVER@", &powershell(HANDED_OVER))
        .replace("@ARTIFACTS@", &artifacts.join("\n"))
}

const SHELL_SCRIPT: &str = r#"#!/bin/sh
set -eu
backend=@BACKEND@
release=@RELEASE@
installation=@INSTALLATION@
base=@BASE@
pairing_prefix=@PAIRING@
paired_prefix=@PAIRED@
removal_prefix=@REMOVAL@
no_autostart_prefix=@NO_AUTOSTART@
handed_over=@HANDED_OVER@
case "$(uname -s):$(uname -m)" in
  Darwin:arm64) target=aarch64-apple-darwin ;;
  Darwin:x86_64) target=x86_64-apple-darwin ;;
  Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-musl ;;
  Linux:x86_64) target=x86_64-unknown-linux-musl ;;
  *)
    echo 'Unsupported runner platform' >&2
    exit 1
    ;;
esac
case "$target" in
@CASES@
  *)
    echo "This server has no Jush runner for $target." >&2
    exit 1
    ;;
esac
# The installation's directory: the one JUSH_HOME names, else this backend's
# own under ~/.jush/runner (`crates/runner/src/console.rs`). Everything below, the launcher, the runner's service
# registration and its removal command, finds the installation from here.
if [ -n "${JUSH_HOME:-}" ]; then
  state=$JUSH_HOME
else
  case "$installation" in
    ''|*[!a-zA-Z0-9._-]*)
      echo 'Invalid installation ID' >&2
      exit 1
      ;;
  esac
  state="$HOME/.jush/runner/$installation"
fi
# Whose installation this is, before anything in it changes: the backend it
# records, compared as written.
if [ -f "$state/backend-url" ]; then
  IFS= read -r existing_backend < "$state/backend-url"
  if [ "$existing_backend" != "$backend" ]; then
    echo 'Installation belongs to another backend' >&2
    exit 1
  fi
fi
# The installation's files are the user's alone; the runner works with the
# mask of the shell this installer runs in (`crates/runner-shell/src/process_builtins.rs`).
user_umask=$(umask)
umask 077
mkdir -p "$state/releases"
# Absolute, as the runner and the lines this installer shows name it.
state=$(CDPATH= cd -- "$state" && pwd)
if ! mkdir "$state/install.lock" 2>/dev/null; then
  echo 'Another installer is active for this installation' >&2
  exit 1
fi
stage=
cleanup() {
  if [ -n "$stage" ]; then
    rm -r "$stage"
  fi
  rmdir "$state/install.lock"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
stage=$(mktemp -d "$state/releases/.download-XXXXXX")
verify() {
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$1")
  else
    actual=$(shasum -a 256 "$1")
  fi
  actual=${actual%% *}
  if [ "$actual" != "$2" ]; then
    echo 'Runner download checksum mismatch' >&2
    exit 1
  fi
}
curl -fSL "$base/runner-artifacts/$release/$target/jush-runner" -o "$stage/jush-runner"
verify "$stage/jush-runner" "$runner_hash"
chmod 755 "$stage/jush-runner"
pid=
log="$state/runner.log"
previous="$state/runner.log.1"
# The size of the file "$1"; 0 when there is none.
size_of() {
  if [ -f "$1" ]; then
    echo $(($(wc -c < "$1")))
  else
    echo 0
  fi
}
# Where the log ended when this installer began waiting, whether it started
# the runner or found it running: what it held before is an earlier
# runner's, or an earlier state's. The size of runner.log.1 tells when the
# log was moved there.
seen=0
previous_size=$(size_of "$previous")
if JUSH_HOME="$state" JUSH_RELEASE_ID="$release" "$stage/jush-runner" status --backend "$backend" >/dev/null 2>&1; then
  echo "Runner already running: $state"
  # It wrote its state before this installer began: it writes it again,
  # its code or the device it is paired as, to its log alone. A runner that
  # ended meanwhile is found stopped below.
  seen=$(size_of "$log")
  JUSH_HOME="$state" "$stage/jush-runner" announce --backend "$backend" >/dev/null 2>&1 || true
else
  # Not `status`, which zsh keeps read-only.
  runner_status=$?
  if [ "$runner_status" -eq 3 ]; then
    echo 'Waiting for existing jobs before upgrading this runner…'
    JUSH_HOME="$state" "$stage/jush-runner" drain --backend "$backend"
  fi
  # Every release runs from one file, so the system tells one program across
  # updates (`crates/runner-protocol/src/installation.rs`).
  mv -f "$stage/jush-runner" "$state/jush-runner"
  # Pass values as quoted arguments; never interpolate a backend into executable shell code.
  cat > "$state/run.next" <<'LAUNCHER'
@LAUNCHER@
LAUNCHER
  printf '%s\n' "$backend" > "$state/backend-url"
  printf '%s\n' "$release" > "$state/release-id"
  chmod 755 "$state/run.next"
  mv "$state/run.next" "$state/run"
  seen=$(size_of "$log")
  # The runner writes its log itself, and leaves this terminal's session as
  # it starts, so that interrupting this installer, or closing its terminal,
  # leaves the runner running. Until then, as a background command of a
  # shell without job control, it ignores the terminal's interrupt, and
  # under nohup its closing. Never `set -m`: a shell with job control hands
  # the terminal back as it turns it off, to the group that had it when
  # this installer started, which under `curl | sh` was curl's and is gone
  # by then; dash then exits with an error.
  nohup sh -c 'umask "$1" && exec "$2" run --log' sh "$user_umask" "$state/run" > /dev/null 2>&1 < /dev/null &
  pid=$!
  printf 'Runner installed for %s\n' "$backend"
fi
# The installation is in place; the runner's own updates may take its lock
# while this installer waits for pairing.
trap - EXIT
cleanup
# Shows the runner's log as pairing goes: why the device will not start the
# runner by itself, when it will not, each pairing code the runner
# receives, then the device's name and the removal command once it is
# paired. Only complete lines are read; of the codes read at once, only the
# last is still live.
# Reads the complete lines of the file "$1" from byte $seen on and moves
# $seen past them, keeping the last code, name, removal command and reason
# they name.
read_lines() {
  size=$(size_of "$1")
  lines=0
  if [ "$size" -gt "$seen" ]; then
    lines=$(($(tail -c +"$((seen + 1))" "$1" | head -c "$((size - seen))" | wc -l)))
  fi
  if [ "$lines" -gt 0 ]; then
    chunk=$(tail -c +"$((seen + 1))" "$1" | head -n "$lines")
    seen=$((seen + $(tail -c +"$((seen + 1))" "$1" | head -n "$lines" | wc -c)))
    while IFS= read -r line; do
      case "$line" in
        "$pairing_prefix"*) code=${line#"$pairing_prefix"} ;;
        "$paired_prefix"*) name=${line#"$paired_prefix"} ;;
        "$removal_prefix"*) removal=${line#"$removal_prefix"} ;;
        "$no_autostart_prefix"*) unstarted=${line#"$no_autostart_prefix"} ;;
        "$handed_over"*) [ -n "$handed" ] || handed=$(date +%s) ;;
      esac
    done <<CHUNK
$chunk
CHUNK
  fi
}
begin=$seen
shown=
unstarted=
# When the runner this installer started handed over to the service
# manager's (`crates/runner/src/autostart.rs`), from which on the
# installation's own status says whether a runner runs.
handed=
while :; do
  code=
  name=
  removal=
  # The runner started its log again past its size: what was left in the
  # old one, runner.log.1 by now, comes first, then the new one from its
  # start.
  previous_now=$(size_of "$previous")
  if [ "$(size_of "$log")" -lt "$seen" ] || [ "$previous_now" -ne "$previous_size" ]; then
    read_lines "$previous"
    seen=0
    begin=0
  fi
  previous_size=$previous_now
  read_lines "$log"
  if [ -n "$unstarted" ]; then
    printf 'This device will not start Jush by itself after a restart: %s\n' "$unstarted"
    unstarted=
  fi
  if [ -n "$removal" ]; then
    printf 'Paired as %s\nTo remove this runner, run: %s\n' "$name" "$removal"
    exit 0
  fi
  if [ -n "$code" ] && [ "$code" != "$shown" ]; then
    if [ -z "$shown" ]; then
      printf 'Enter this pairing code in Add Device: %s\n' "$code"
    else
      printf 'The code expired; enter this one instead: %s\n' "$code"
    fi
    shown=$code
  fi
  if [ -n "$pid" ]; then
    alive=$(kill -0 "$pid" 2>/dev/null && echo yes || echo no)
    # It may have handed over after the log was read; the lines stay for
    # the next round to read.
    if [ "$alive" = no ] && [ -z "$handed" ] && [ -f "$log" ] &&
      tail -c +"$((begin + 1))" "$log" | grep -qF "$handed_over"; then
      handed=$(date +%s)
    fi
    if [ -n "$handed" ]; then
      pid=
    fi
  fi
  if [ -z "$pid" ]; then
    alive=$("$state/run" status >/dev/null 2>&1 && echo yes || echo no)
    # The manager's runner takes the installation a moment after the
    # handover.
    if [ "$alive" = no ] && [ -n "$handed" ] && [ $(($(date +%s) - handed)) -lt 15 ]; then
      alive=yes
    fi
  fi
  if [ "$alive" = no ]; then
    echo 'The runner stopped:' >&2
    if [ -f "$log" ]; then
      tail -c +"$((begin + 1))" "$log" >&2
    fi
    exit 1
  fi
  sleep 0.2
done
"#;

const POWERSHELL_SCRIPT: &str = r#"#requires -Version 5.1
$ErrorActionPreference = 'Stop'
$jushBackend = @BACKEND@
$jushBase = @BASE@
$jushRelease = @RELEASE@
$jushInstallation = @INSTALLATION@
$jushPairingPrefix = @PAIRING@
$jushPairedPrefix = @PAIRED@
$jushRemovalPrefix = @REMOVAL@
$jushNoAutostartPrefix = @NO_AUTOSTART@
$jushHandedOver = @HANDED_OVER@
# When the runner this installer started handed over to the service
# manager's (`crates/runner/src/autostart.rs`).
$jushHanded = $null
$jushArtifacts = @{
@ARTIFACTS@
}
switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
  'Arm64' { $jushTarget = 'aarch64-pc-windows-msvc' }
  'X64' { $jushTarget = 'x86_64-pc-windows-msvc' }
  default { throw 'Unsupported runner architecture' }
}
# A server's release may lack this platform's runner, as a development
# build of other targets does; the shell installer refuses in the same words.
if (-not $jushArtifacts.ContainsKey($jushTarget)) { throw "This server has no Jush runner for $jushTarget." }
$jushHome = if ($env:USERPROFILE) { $env:USERPROFILE } else { [Environment]::GetFolderPath('UserProfile') }
# The installation's directory: the one JUSH_HOME names, else this backend's
# own under ~/.jush/runner (`crates/runner/src/console.rs`). Everything below, the launcher, the runner's service
# registration and its removal command, finds the installation from here.
if ($env:JUSH_HOME) {
  # Absolute, as the runner and the lines this installer shows name it; a
  # relative one is read against this session's location.
  $jushState = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($env:JUSH_HOME)
} else {
  if ($jushInstallation -notmatch '^[a-zA-Z0-9._-]+$') { throw 'Invalid installation ID' }
  $jushState = Join-Path $jushHome ".jush/runner/$jushInstallation"
}
$jushBackendFile = Join-Path $jushState 'backend-url'
# Whose installation this is, before anything in it changes: the backend it
# records, compared as written (`-cne`: `-ne` ignores case, and a path's case
# can name another backend).
if ((Test-Path $jushBackendFile) -and ([IO.File]::ReadAllText($jushBackendFile).Trim() -cne $jushBackend)) {
  throw 'Installation belongs to another backend'
}
$jushReleases = Join-Path $jushState 'releases'
[IO.Directory]::CreateDirectory($jushReleases) | Out-Null
$jushLockFile = Join-Path $jushState 'install.lock'
$jushLock = [IO.File]::Open($jushLockFile, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::Write, [IO.FileShare]::None)
$jushStage = Join-Path $jushReleases ('.download-' + [Guid]::NewGuid().ToString('N'))
$jushExe = Join-Path $jushStage 'jush-runner.exe'
$jushPreviousHome = $env:JUSH_HOME
$jushPreviousRelease = $env:JUSH_RELEASE_ID
$jushProcess = $null
$jushLog = Join-Path $jushState 'runner.log'
$jushPrevious = Join-Path $jushState 'runner.log.1'
# The size of the file at `$Path`; 0 when there is none.
function Get-JushSize([string]$Path) {
  if (Test-Path $Path) { (Get-Item $Path).Length } else { [long]0 }
}
# Where the log ended when this installer began waiting, whether it started
# the runner or found it running: what it held before is an earlier
# runner's, or an earlier state's. The size of runner.log.1 tells when the
# log was moved there.
$jushSeen = [long]0
$jushPreviousSize = Get-JushSize $jushPrevious
# The bytes of the file at `$Path` from `$From` on, read beside the runner,
# which writes it and may rename it.
function Read-JushLog([string]$Path, [long]$From) {
  # The comma keeps an array, even an empty one, from being unrolled.
  if (-not (Test-Path $Path)) { return ,[byte[]]::new(0) }
  $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
  try {
    $bytes = [byte[]]::new([Math]::Max(0, $stream.Length - $From))
    $stream.Position = [Math]::Min($From, $stream.Length)
    $read = 0
    while ($read -lt $bytes.Length) {
      $count = $stream.Read($bytes, $read, $bytes.Length - $read)
      if ($count -eq 0) { break }
      $read += $count
    }
    [Array]::Resize([ref]$bytes, $read)
    return ,$bytes
  } finally {
    $stream.Dispose()
  }
}
# Starts the runner out of the job this installer's session shares, which
# OpenSSH ends, with every process in it, when the session ends: a runner
# installed over SSH stopped as the installer returned. A job that allows
# no breakaway refuses that start, and the runner then starts in it, as in
# a PowerShell window, whose job nothing closes. Start-Process takes no
# creation flags, so the installer calls CreateProcess itself.
function Start-JushRunner([string]$CommandLine, [string]$Directory) {
  if (-not ('JushRunnerStart' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class JushRunnerStart {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct StartupInfo {
    public int cb;
    public IntPtr reserved, desktop, title;
    public int x, y, width, height, columns, rows, fill, flags;
    public short show, reserved2;
    public IntPtr reserved3, input, output, error;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct ProcessInformation {
    public IntPtr process, thread;
    public int id, threadId;
  }
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern bool CreateProcessW(string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInformation information);
  [DllImport("kernel32.dll")]
  static extern bool CloseHandle(IntPtr handle);
  // A hidden console of its own, no part in this window's Ctrl+C.
  const uint Hidden = 0x08000000 | 0x00000200;
  const uint Breakaway = 0x01000000;
  const int AccessDenied = 5;
  public static int Start(string commandLine, string directory) {
    ProcessInformation information;
    if (!Create(commandLine, directory, Hidden | Breakaway, out information)) {
      if (Marshal.GetLastWin32Error() != AccessDenied || !Create(commandLine, directory, Hidden, out information)) {
        throw new Win32Exception();
      }
    }
    CloseHandle(information.thread);
    CloseHandle(information.process);
    return information.id;
  }
  static bool Create(string commandLine, string directory, uint flags, out ProcessInformation information) {
    StartupInfo startup = new StartupInfo();
    startup.cb = Marshal.SizeOf(startup);
    return CreateProcessW(null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, false, flags, IntPtr.Zero, directory, ref startup, out information);
  }
}
'@
  }
  [Diagnostics.Process]::GetProcessById([JushRunnerStart]::Start($CommandLine, $Directory))
}
function Invoke-JushControl([string]$Action) {
  $control = [Diagnostics.ProcessStartInfo]::new()
  $control.FileName = $jushExe
  $control.Arguments = $Action + ' --backend "' + $jushBackend + '"'
  $control.UseShellExecute = $false
  $control.CreateNoWindow = $true
  $control.RedirectStandardOutput = $true
  $control.RedirectStandardError = $true
  $process = [Diagnostics.Process]::Start($control)
  try {
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $output.GetAwaiter().GetResult() | Out-Null
    $errors.GetAwaiter().GetResult() | Out-Null
    return $process.ExitCode
  } finally {
    $process.Dispose()
  }
}
# The folder Git for Windows is installed in, as the runner finds it
# (`crates/runner/src/git.rs`), or $null when it finds none.
function Get-JushGit {
  $git = [Diagnostics.ProcessStartInfo]::new()
  $git.FileName = $jushExe
  $git.Arguments = 'git'
  $git.UseShellExecute = $false
  $git.CreateNoWindow = $true
  $git.RedirectStandardOutput = $true
  $process = [Diagnostics.Process]::Start($git)
  try {
    $output = $process.StandardOutput.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -eq 0) { return $output.Trim() }
    return $null
  } finally {
    $process.Dispose()
  }
}
# Installs Git for Windows, whose `usr\bin` holds the utilities a job runs
# besides Jush's own, with its own installer, silently. An account in the
# Administrators group installs it for every user, without asking: the
# installer raises the one UAC prompt. A standard account chooses when asked:
# for every user, with an administrator's name and password at the UAC
# prompt, or for this user alone in its own folder, with no prompt; a window
# that cannot ask installs for this user alone. The installer's download is
# checked by the digest GitHub publishes for it and by its signature.
function Install-JushGit {
  # The group's claim, not its membership: an unelevated process of an
  # administrator holds the group for denial only, which its groups omit.
  $everyone = @([Security.Principal.WindowsIdentity]::GetCurrent().Claims | Where-Object { $_.Value -eq 'S-1-5-32-544' }).Count -gt 0
  if (-not $everyone) {
    try {
      $choices = @(
        [Management.Automation.Device.ChoiceDescription]::new('&Every user', 'Asks for an administrator''s name and password.'),
        [Management.Automation.Device.ChoiceDescription]::new('&Me only', 'Installs in your own folder and asks for nothing.')
      )
      $everyone = $Device.UI.PromptForChoice('Git for Windows', 'Jush runs the utilities of Git for Windows, such as ls and grep, and it is not installed. Install it for:', $choices, 1) -eq 0
    } catch {
      $everyone = $false
    }
  }
  $kind = if ($jushTarget -eq 'aarch64-pc-windows-msvc') { 'arm64' } else { '64-bit' }
  $installer = Join-Path ([IO.Path]::GetTempPath()) ('git-for-windows-' + [Guid]::NewGuid().ToString('N') + '.exe')
  try {
    Write-Output 'Installing Git for Windows...'
    $jushPreviousProgress = $ProgressPreference
    try {
      $ProgressPreference = 'SilentlyContinue'
      $release = Invoke-RestMethod -UseBasicParsing -Uri 'https://api.github.com/repos/git-for-windows/git/releases/latest' -Headers @{ 'User-Agent' = 'jush-installer' }
      $asset = $release.assets | Where-Object { $_.name -like "Git-*-$kind.exe" } | Select-Object -First 1
      if (-not $asset) { throw 'Git for Windows has no installer for this system' }
      Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url -OutFile $installer
    } finally {
      $ProgressPreference = $jushPreviousProgress
    }
    if ($asset.digest -and (Get-FileHash -Algorithm SHA256 $installer).Hash -ne ($asset.digest -replace '^sha256:', '')) {
      throw 'Git for Windows download checksum mismatch'
    }
    if ((Get-AuthenticodeSignature $installer).Status -ne 'Valid') {
      throw 'Git for Windows installer signature is not valid'
    }
    $options = '/VERYSILENT', '/NORESTART', '/SUPPRESSMSGBOXES'
    if ($everyone) {
      $process = Start-Process -FilePath $installer -ArgumentList $options -Verb RunAs -Wait -PassThru
    } else {
      $folder = Join-Path $env:LOCALAPPDATA 'Programs\Git'
      $process = Start-Process -FilePath $installer -ArgumentList ($options + ('/DIR="' + $folder + '"')) -Wait -PassThru
    }
    if ($process.ExitCode -ne 0) { throw "its installer exited with code $($process.ExitCode)" }
  } finally {
    if (Test-Path $installer) { Remove-Item -Force $installer }
  }
}
try {
  try {
    [IO.Directory]::CreateDirectory($jushStage) | Out-Null
    Write-Output "Downloading runner for $jushTarget..."
    $jushPreviousProgress = $ProgressPreference
    try {
      $ProgressPreference = 'SilentlyContinue'
      Invoke-WebRequest -UseBasicParsing -Uri "$jushBase/runner-artifacts/$jushRelease/$jushTarget/jush-runner.exe" -OutFile $jushExe
    } finally {
      $ProgressPreference = $jushPreviousProgress
    }
    if ((Get-Item $jushExe).Length -ne $jushArtifacts[$jushTarget].Size -or
        (Get-FileHash -Algorithm SHA256 $jushExe).Hash.ToLowerInvariant() -ne $jushArtifacts[$jushTarget].Hash) {
      throw 'Runner download checksum mismatch'
    }
    # A job's utilities besides Jush's own are Git for Windows': a device is
    # not added without it, so a failed or refused install ends the setup.
    if (-not (Get-JushGit)) {
      try {
        Install-JushGit
      } catch {
        throw "Git for Windows was not installed: $($_.Exception.Message)"
      }
    }
    $env:JUSH_HOME = $jushState
    $env:JUSH_RELEASE_ID = $jushRelease
    $jushStatus = Invoke-JushControl 'status'
    if ($jushStatus -eq 0) {
      Write-Output "Runner already running: $jushState"
      # It wrote its state before this installer began: it writes it again,
      # its code or the device it is paired as, to its log alone. A runner
      # that ended meanwhile is found stopped below.
      $jushSeen = Get-JushSize $jushLog
      Invoke-JushControl 'announce' | Out-Null
    } else {
      if ($jushStatus -eq 3) {
        Write-Output 'Waiting for existing jobs before upgrading this runner...'
        if ((Invoke-JushControl 'drain') -ne 0) { throw 'Runner drain failed' }
      }
      # Every release runs from one file, so the system tells one program
      # across updates (`crates/runner-protocol/src/installation.rs`).
      $jushRunner = Join-Path $jushState 'jush-runner.exe'
      # A drained runner releases its lock before its process ends, and
      # Windows replaces no running executable but renames one: the earlier
      # runner is set aside, as an update does (`crates/runner/src/update.rs`).
      if (Test-Path $jushRunner) {
        Move-Item $jushRunner (Join-Path $jushReleases ('.replaced-' + [Guid]::NewGuid().ToString('N') + '.exe'))
      }
      Move-Item $jushExe $jushRunner
      $jushExe = $jushRunner
      $jushUtf8 = [Text.UTF8Encoding]::new($false)
      [IO.File]::WriteAllText($jushBackendFile, $jushBackend + [Environment]::NewLine, $jushUtf8)
      [IO.File]::WriteAllText((Join-Path $jushState 'release-id'), $jushRelease + [Environment]::NewLine, $jushUtf8)
      $jushLauncher = @'
@LAUNCHER@
'@
      [IO.File]::WriteAllText((Join-Path $jushState 'run.ps1'), $jushLauncher, $jushUtf8)
      Write-Output 'Starting runner...'
      $jushSeen = Get-JushSize $jushLog
      # The runner writes its log itself.
      $jushProcess = Start-JushRunner ('"' + $jushExe + '" run --backend "' + $jushBackend + '" --log') $jushHome
      Write-Output "Runner installed for $jushBackend"
    }
  } catch {
    if ($jushProcess -and -not $jushProcess.HasExited) {
      $jushProcess.Kill()
      $jushProcess.WaitForExit()
    }
    throw
  } finally {
    # The installation is in place; the runner's own updates may take its
    # lock while this installer waits for pairing.
    $jushLock.Dispose()
    if (Test-Path $jushStage) { Remove-Item -Recurse -Force $jushStage }
  }
  # Shows the runner's log as pairing goes: each pairing code the runner
  # receives, then the device's name and the removal command once it is
  # paired. Only complete lines are read; of the codes read at once, only
  # the last is still live. Interrupting this installer leaves the runner
  # running.
  $jushBegin = $jushSeen
  $jushShown = $null
  while ($true) {
    $jushCode = $null
    $jushName = $null
    $jushRemoval = $null
    # The runner started its log again past its size: what was left in the
    # old one, runner.log.1 by now, comes first, then the new one from its
    # start.
    $jushPaths = @($jushLog)
    $jushPreviousNow = Get-JushSize $jushPrevious
    if ((Get-JushSize $jushLog) -lt $jushSeen -or $jushPreviousNow -ne $jushPreviousSize) {
      $jushPaths = @($jushPrevious, $jushLog)
    }
    $jushPreviousSize = $jushPreviousNow
    foreach ($jushPath in $jushPaths) {
      if ($jushPath -eq $jushLog -and $jushPaths.Count -eq 2) {
        $jushSeen = [long]0
        $jushBegin = [long]0
      }
      $jushBytes = Read-JushLog $jushPath $jushSeen
      $jushEnd = if ($jushBytes.Length -gt 0) { [Array]::LastIndexOf($jushBytes, [byte]10) } else { -1 }
      if ($jushEnd -lt 0) { continue }
      $jushSeen += $jushEnd + 1
      foreach ($jushLine in [Text.Encoding]::UTF8.GetString($jushBytes, 0, $jushEnd).Split("`n")) {
        $jushLine = $jushLine.TrimEnd("`r")
        if ($jushLine.StartsWith($jushPairingPrefix)) {
          $jushCode = $jushLine.Substring($jushPairingPrefix.Length)
        } elseif ($jushLine.StartsWith($jushPairedPrefix)) {
          $jushName = $jushLine.Substring($jushPairedPrefix.Length)
        } elseif ($jushLine.StartsWith($jushRemovalPrefix)) {
          $jushRemoval = $jushLine.Substring($jushRemovalPrefix.Length)
        } elseif ($jushLine.StartsWith($jushHandedOver)) {
          if (-not $jushHanded) { $jushHanded = [DateTime]::UtcNow }
        } elseif ($jushLine.StartsWith($jushNoAutostartPrefix)) {
          Write-Output ('This device will not start Jush by itself after a restart: ' + $jushLine.Substring($jushNoAutostartPrefix.Length))
        }
      }
    }
    if ($jushRemoval) {
      Write-Output "Paired as $jushName"
      Write-Output "To remove this runner, run: $jushRemoval"
      break
    }
    if ($jushCode -and $jushCode -ne $jushShown) {
      if ($jushShown) {
        Write-Output "The code expired; enter this one instead: $jushCode"
      } else {
        Write-Output "Enter this pairing code in Add Device: $jushCode"
      }
      $jushShown = $jushCode
    }
    # The runner it started may have handed over after the log was read.
    if ($jushProcess -and $jushProcess.HasExited -and -not $jushHanded) {
      $jushTail = [Text.Encoding]::UTF8.GetString((Read-JushLog $jushLog $jushBegin))
      if ($jushTail.Contains($jushHandedOver)) { $jushHanded = [DateTime]::UtcNow }
    }
    $jushAlive = if ($jushProcess -and -not $jushHanded) {
      -not $jushProcess.HasExited
    } else {
      ((Invoke-JushControl 'status') -eq 0) -or ($jushHanded -and ([DateTime]::UtcNow - $jushHanded).TotalSeconds -lt 15)
    }
    if (-not $jushAlive) {
      throw ('The runner stopped:' + [Environment]::NewLine + [Text.Encoding]::UTF8.GetString((Read-JushLog $jushLog $jushBegin)))
    }
    Start-Sleep -Milliseconds 200
  }
} finally {
  $env:JUSH_HOME = $jushPreviousHome
  $env:JUSH_RELEASE_ID = $jushPreviousRelease
  if ($jushProcess) { $jushProcess.Dispose() }
}
"#;
