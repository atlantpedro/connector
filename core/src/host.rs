//! Programs on the computer: started, found and looked for. Inside the Flatpak the editor runs
//! outside the sandbox, which sees neither its programs nor its processes, so everything goes
//! through `flatpak-spawn --host` there.

use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// True when running as a Flatpak.
pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

/// A command that runs program on the host.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    if in_flatpak() {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.arg("--host").arg(program);
        cmd
    } else {
        Command::new(program)
    }
}

/// Start a program and leave it running on its own. A thread waits for it, so that no
/// zombie is left behind once it exits.
pub fn spawn_detached(program: &str, args: &[&OsStr]) -> io::Result<()> {
    let mut cmd = command(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    log::info!("launching {program} {args:?}");
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// True if the command ran and succeeded; its output is thrown away.
pub fn succeeds(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// True if a process with one of these names runs.
pub fn process_running(names: &[&str]) -> bool {
    if in_flatpak() {
        // pgrep matches the first 15 characters of the name, which is all the kernel keeps
        return names.iter().any(|name| succeeds(command("pgrep").args(["-x", name])));
    }
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system
        .processes()
        .values()
        .any(|p| names.iter().any(|n| p.name().to_string_lossy().eq_ignore_ascii_case(n)))
}

/// Where a program on PATH lives.
pub fn which(program: &str) -> Option<PathBuf> {
    if in_flatpak() {
        let output = command("sh").args(["-c", "command -v \"$1\"", "sh", program]).stderr(Stdio::null()).output().ok()?;
        let path = String::from_utf8(output.stdout).ok()?;
        let path = path.trim();
        return (output.status.success() && path.starts_with('/')).then(|| PathBuf::from(path));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|p| p.is_file())
}
