fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
    native_media();
}

/// Builds Horizon's small C shims over PipeWire, JPEG XL and the
/// libretro log callback (`csrc/`),
/// against the libraries the Flatpak runtime ships. Struct layouts stay in
/// C, compiled with the SDK's headers, so no bindings are generated.
fn native_media() {
    let libraries = ["libpipewire-0.3", "libjxl"];
    let mut build = cc::Build::new();
    let mut found = Vec::new();
    for name in libraries {
        // Link flags are printed after the shim, which needs these libraries:
        // the linker resolves left to right.
        let library = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(name)
            .unwrap_or_else(|error| panic!("{name} is required to build Horizon: {error}"));
        build.includes(&library.include_paths);
        found.push(library);
    }
    build
        .file("csrc/horizon_audio.c")
        .file("csrc/horizon_jxl.c")
        .file("csrc/horizon_retro_log.c")
        .flag_if_supported("-std=gnu11")
        .warnings(true)
        .compile("horizon_native");
    for library in found {
        for path in library.link_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
        for name in library.libs {
            println!("cargo:rustc-link-lib={name}");
        }
    }
    println!("cargo:rerun-if-changed=csrc");
}
