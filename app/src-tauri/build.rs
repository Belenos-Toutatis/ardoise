fn main() {
    // The built interface is embedded in the executable: rebuild whenever `npm run build`
    // changes it (cargo scans the whole folder for modifications).
    println!("cargo:rerun-if-changed=../dist");
    delay_load_media_foundation();
    tauri_build::build();
}

/// Media Foundation (the MP4 export, `ardoise-media`) is delay-loaded: its DLLs are only looked
/// for at the first call into them, not when the executable starts. Windows N editions without
/// the Media Feature Pack have no `mfplat.dll` or `mfreadwrite.dll`; with ordinary imports
/// `ardoise.exe` would not start at all there (docs/research/media-stack.md §7c), with
/// delay-loading only the MP4 export changes (`ardoise_media::media_foundation_available`
/// checks first, then the OpenH264 fallback is offered).
///
/// Passed as link arguments of the executable only (not through `RUSTFLAGS`, which would
/// replace the static C runtime flag of `.cargo/config.toml`). `delayimp.lib` holds the MSVC
/// delay-load helper. While nothing in the executable imports these DLLs the linker ignores
/// the options (warning LNK4199, silenced: the imports appear once `ardoise-media` is linked).
fn delay_load_media_foundation() {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") == "windows" && var("CARGO_CFG_TARGET_ENV") == "msvc" {
        for dll in ["mfplat.dll", "mfreadwrite.dll"] {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
        println!("cargo:rustc-link-arg-bins=/IGNORE:4199");
    }
}
