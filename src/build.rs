//! Embeds the app icon and version info into craftpulse.exe on Windows.

fn main() {
    println!("cargo:rerun-if-changed=src/build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "CraftPulse");
        resource.set("FileDescription", "CraftPulse");
        resource.set("LegalCopyright", "Copyright (c) 2026 egsk4");
        if let Err(error) = resource.compile() {
            // A missing icon shouldn't stop the app from building.
            println!("cargo:warning=could not embed the exe icon: {error}");
        }
    }
}
