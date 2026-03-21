fn main() {
    napi_build::setup();

    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/drag_mac.m")
            .flag("-fobjc-arc")
            .compile("drag_mac");

        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
}
