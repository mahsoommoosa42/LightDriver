fn main() {
    println!("cargo:rerun-if-changed=../../assets/branding/lightdriver.ico");
    println!("cargo:rerun-if-changed=../../assets/branding/windows-resource.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/branding/lightdriver.ico")
            .set("ProductName", "LightDriver")
            .set(
                "FileDescription",
                if std::env::var("CARGO_PKG_NAME").as_deref() == Ok("lightdriver-fan-debug") {
                    "LightDriver Firmware Fan Lab"
                } else {
                    "LightDriver Workbench"
                },
            )
            .compile()
            .expect("compile native LightDriver icon resource");
    }
}
