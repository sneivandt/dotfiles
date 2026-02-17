use std::process::Command;

fn main() {
    // Embed the git tag or describe as the version
    if let Ok(output) = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        && output.status.success()
    {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        println!("cargo:rustc-env=DOTFILES_VERSION={version}");
    }

    // Re-run if git HEAD changes
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/");
}
