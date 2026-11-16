use std::path::PathBuf;

fn main() {
    let resources = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("resources");
    println!("cargo:rerun-if-changed={}", resources.join("app.ico").display());
    println!("cargo:rerun-if-changed={}", resources.join("app.manifest").display());
    build_common::compile_rc(&resources.join("demo.rc"));
}
