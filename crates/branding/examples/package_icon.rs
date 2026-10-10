fn main() -> std::io::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/branding/lightdriver.ico");
    std::fs::write(&path, lightdriver_branding::packaged_icon())?;
    println!("Packaged approved logo: {}", path.display());
    Ok(())
}
