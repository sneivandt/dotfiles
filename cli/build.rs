use std::process::Command;

fn main() {
    // Prefer DOTFILES_VERSION env var if set (e.g., by CI release workflow),
    // otherwise fall back to git describe for local development builds.
    if let Ok(version) = std::env::var("DOTFILES_VERSION") {
        println!("cargo:rustc-env=DOTFILES_VERSION={version}");
    } else if let Ok(output) = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        && output.status.success()
    {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        println!("cargo:rustc-env=DOTFILES_VERSION={version}");
    }

    // Re-run if git HEAD changes or env var changes
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/");
    println!("cargo:rerun-if-env-changed=DOTFILES_VERSION");
}
