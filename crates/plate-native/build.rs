fn main() {
    let ocio = pkg_config::Config::new()
        .atleast_version("2.2")
        .probe("OpenColorIO")
        .expect("OpenColorIO >= 2.2 not found");
    let oiio = pkg_config::Config::new()
        .atleast_version("2.4")
        .probe("OpenImageIO")
        .expect("OpenImageIO >= 2.4 not found");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let paths: std::collections::BTreeSet<_> = ocio.link_paths.iter().chain(oiio.link_paths.iter()).collect();
        for path in paths {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
        }
    }

    let mut build = cxx_build::bridge("src/lib.rs");
    build.file("cpp/shim.cpp").std("c++17").flag_if_supported("-O2");
    for path in ocio.include_paths.iter().chain(oiio.include_paths.iter()) {
        build.include(path);
    }
    build.compile("plate_native_shim");

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    println!("cargo:rerun-if-changed=cpp/shim.h");
}
