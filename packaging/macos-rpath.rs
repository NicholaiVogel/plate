// Homebrew uses absolute install names, but conda-forge uses @rpath.
// Apply this to the app, core tests and decoding examples.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    let mut paths = std::collections::BTreeSet::new();
    for package in ["OpenColorIO", "OpenImageIO", "libavformat"] {
        let lib = pkg_config::Config::new().cargo_metadata(false).probe(package)
            .unwrap_or_else(|e| panic!("{package}: {e}"));
        paths.extend(lib.link_paths);
    }
    for path in paths {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
    }
}
