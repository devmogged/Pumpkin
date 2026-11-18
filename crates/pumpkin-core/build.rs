use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    // Get short hash (7 chars) for display
    let git_hash_short =
        git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // Get full hash for hover text
    let git_hash_full = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let rustc_version = match Command::new(rustc).arg("--version").output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .trim()
            .strip_prefix("rustc ")
            .unwrap_or("")
            .to_string(),
        _ => "unknown".to_string(),
    };

    // Ask git where HEAD and the refs live: in a submodule or worktree `.git` is a file, and
    // watching a path that doesn't exist makes Cargo rebuild this crate on every build.
    for path in ["HEAD", "refs/heads", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", path])
            && std::path::Path::new(&path).exists()
        {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    println!("cargo::rustc-env=GIT_HASH={git_hash_short}");
    println!("cargo::rustc-env=GIT_HASH_FULL={git_hash_full}");
    println!("cargo::rustc-env=RUSTC_VERSION={rustc_version}");
}
