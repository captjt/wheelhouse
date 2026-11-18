//! The web app is built into the binary from web/dist. A checkout that
//! hasn't built it yet gets a placeholder page, so `cargo build` always works.

fn main() {
    let dist = std::path::Path::new("web/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        std::fs::create_dir_all(dist).expect("creating web/dist");
        std::fs::write(&index, "<!doctype html><title>Wheelhouse</title><p>The web app isn't built. Run <code>just web</code>, then build again.</p>\n")
            .expect("writing web/dist/index.html");
    }
    println!("cargo:rerun-if-changed=web/dist");
    // The commit it's built from, so `wh doctor` can tell a running hub
    // that's older than the `wh` you just installed.
    let git =
        |args: &[&str]| std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let commit = git(&["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=WH_COMMIT={commit}");
    // And its branch, for the build Wheelhouse Dev shows on its badge.
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=WH_BRANCH={branch}");
    // Build again when HEAD moves. In a worktree `.git` is a file: ask git
    // where its HEAD and the shared refs are.
    let head = git(&["rev-parse", "--git-path", "HEAD"]).unwrap_or_else(|| ".git/HEAD".into());
    let refs = git(&["rev-parse", "--git-common-dir"]).map(|d| format!("{d}/refs/heads")).unwrap_or_else(|| ".git/refs/heads".into());
    for p in [head, refs] {
        if std::path::Path::new(&p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
}
