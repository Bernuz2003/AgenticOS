use std::path::Path;
use std::process::Command;

pub(crate) fn run_with_timeout(
    cwd: &Path,
    program: &str,
    args: &[String],
    timeout_s: u64,
) -> Result<std::process::Output, String> {
    let mut wrapped = Command::new("timeout");
    wrapped
        .arg("--signal=KILL")
        .arg(format!("{}s", timeout_s.max(1)))
        .arg(program);
    for arg in args {
        wrapped.arg(arg);
    }
    wrapped.current_dir(cwd).output().map_err(|err| {
        format!(
            "SysCall Error: Failed to execute '{}' via timeout wrapper: {}",
            program, err
        )
    })
}
