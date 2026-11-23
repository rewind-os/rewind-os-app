//! Embeds the icon, version information and the application manifest into the Windows
//! executable. Only runs when building on Windows for Windows, where the resource compiler
//! is available.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/rewindows.ico");
    println!("cargo:rerun-if-changed=rewindows.manifest");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if !(target_windows && cfg!(windows)) {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/rewindows.ico");
    res.set("ProductName", "ReWindows");
    res.set("FileDescription", "ReWindows");
    res.set_manifest_file("rewindows.manifest");
    if let Err(err) = res.compile() {
        println!("cargo:warning=could not embed Windows resources: {err}");
    }
}
