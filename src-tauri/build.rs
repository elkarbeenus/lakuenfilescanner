fn main() {
    println!("cargo:rerun-if-env-changed=LAKUEN_VERSION_LABEL");
    tauri_build::build()
}
