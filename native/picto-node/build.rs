fn main() {
    napi_build::setup();

    println!("cargo:rerun-if-changed=src/drag_mac.m");
    println!("cargo:rerun-if-changed=src/open_with_mac.m");

    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/drag_mac.m")
            .file("src/open_with_mac.m")
            .flag("-fobjc-arc")
            .compile("drag_mac");

        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=CoreServices");
    }
}
