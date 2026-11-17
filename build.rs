//! The web app is built into the binary from web/dist. A checkout that
//! hasn't built it yet gets a placeholder page, so `cargo build` always works.

fn main() {
    let dist = std::path::Path::new("web/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        std::fs::create_dir_all(dist).expect("creating web/dist");
        std::fs::write(
            &index,
            "<!doctype html><title>Wheelhouse</title><p>The web app isn't built. Run <code>just web</code>, then build again.</p>\n",
        )
        .expect("writing web/dist/index.html");
    }
    println!("cargo:rerun-if-changed=web/dist");
}
