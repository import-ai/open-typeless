fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/remote_microphone.m");
        cc::Build::new()
            .file("src/remote_microphone.m")
            .flag("-fobjc-arc")
            .compile("remote_microphone");
        for framework in ["CoreBluetooth", "Foundation", "IOKit", "ApplicationServices"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    tauri_build::build()
}
