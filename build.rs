#[path = "src/source_identity.rs"]
mod source_identity;

fn main() -> std::io::Result<()> {
    for path in ["src", "Cargo.toml", "Cargo.lock", "build.rs"] {
        println!("cargo:rerun-if-changed={path}");
    }

    let source = source_identity::digest(std::path::Path::new("."))?;
    println!("cargo:rustc-env=CI_SOURCE_DIGEST={source}");
    Ok(())
}
