//! Valve's library sits beside this program in every bundle. Linux and macOS look for a
//! library beside the program only if the program says so; cargo does not add that. Windows
//! looks beside the program already.

fn main() {
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN"),
        Ok("macos") => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}
