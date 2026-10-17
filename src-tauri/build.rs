fn main() {
    // macOS builds bundle ffmpeg as a sidecar (tauri.macos.conf.json), and
    // tauri-build refuses to build without it. `bun run tauri dev|build` fetch
    // it first; a bare `cargo build`/`cargo test` needs it fetched once by hand.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("-apple-darwin") {
        let sidecar = format!("binaries/ffmpeg-{}", target);
        println!("cargo:rerun-if-changed={}", sidecar);
        if !std::path::Path::new(&sidecar).is_file() {
            panic!(
                "{} is missing. Run `bun run fetch:ffmpeg` from the repository root to download the bundled ffmpeg.",
                sidecar
            );
        }
    }

    tauri_build::build()
}
