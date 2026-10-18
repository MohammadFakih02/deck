use std::path::PathBuf;
use std::process::Command;

fn main() {
    rtss32();
    // Claude Sync tells machines which Deck build is newer
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=DECK_BUILD={ts}");
    tauri_build::build()
}

/// Builds the 32-bit RivaTuner helper (`rtss32/`) and leaves it in OUT_DIR for `include_bytes!`.
/// RivaTuner's on-screen display switch lives in its 32-bit hooks, which 64-bit Deck can't load.
/// Needs the i686-pc-windows-msvc target (`rustup target add i686-pc-windows-msvc`); without it
/// Deck still builds, with an empty helper, and the overlay key leaves RivaTuner's switch alone.
fn rtss32() {
    println!("cargo:rerun-if-changed=rtss32/src/main.rs");
    println!("cargo:rerun-if-changed=rtss32/Cargo.toml");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dest = out.join("rtss-osd32.exe");
    // target/rtss32, not OUT_DIR: OUT_DIR is deep enough that the linker's temp paths pass
    // Windows' 260-character limit (LNK1104) when the repo itself sits in a deep folder
    let target_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("target").join("rtss32");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .args(["build", "--release", "--quiet", "--target", "i686-pc-windows-msvc", "--manifest-path", "rtss32/Cargo.toml", "--target-dir"])
        .arg(&target_dir)
        // the outer build's flags are for 64-bit Deck, not this helper. It links the C runtime in, as
        // Deck does, so it starts on PCs without the Visual C++ runtime.
        .env("CARGO_ENCODED_RUSTFLAGS", "-Ctarget-feature=+crt-static")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let exe = target_dir.join("i686-pc-windows-msvc").join("release").join("rtss-osd32.exe");
    println!("cargo:rerun-if-env-changed=DECK_REQUIRE_RTSS32");
    if built && exe.exists() {
        std::fs::copy(&exe, &dest).unwrap();
    } else if std::env::var("DECK_REQUIRE_RTSS32").is_ok_and(|v| v == "1") {
        // release builds (CI) must not ship without it
        panic!("DECK_REQUIRE_RTSS32=1 but the 32-bit RivaTuner helper didn't build (rustup target add i686-pc-windows-msvc)");
    } else {
        println!("cargo:warning=32-bit RivaTuner helper not built (rustup target add i686-pc-windows-msvc): the overlay key won't flip RivaTuner's own switch");
        std::fs::write(&dest, b"").unwrap();
    }
}
