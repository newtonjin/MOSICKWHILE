fn main() {
    tauri_build::build();
    // Bridge DLL is built by npm `build:bridge` / `tauri build` prep and copied
    // next to the final executable.
    println!("cargo:rerun-if-changed=../bridge/src");
    println!("cargo:rerun-if-changed=../bridge/Cargo.toml");
}
