fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/platform/desktop_status.m");
        cc::Build::new()
            .file("src/platform/desktop_status.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("nano_desktop_status");
        for framework in ["Foundation", "AppKit", "CoreAudio", "IOKit"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    let ios = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios");
    let mobile = ios || std::env::var_os("CARGO_FEATURE_MOBILE").is_some();
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_MOBILE");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    slint_build::compile_with_config(
        if mobile {
            "ui/mobile.slint"
        } else {
            "ui/app.slint"
        },
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("Slint UI must compile");

    if ios {
        println!("cargo:rerun-if-changed=ios/NativeBridge.m");
        println!("cargo:rerun-if-changed=ios/NativeBridge.h");
        cc::Build::new()
            .file("ios/NativeBridge.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .flag("-fmodules")
            .compile("nano_ios_native");
        for framework in [
            "Foundation",
            "UIKit",
            "AVFoundation",
            "MediaPlayer",
            "UniformTypeIdentifiers",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
