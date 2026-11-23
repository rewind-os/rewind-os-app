//! Embeds the icon, version information and the application manifest into the Windows
//! executable. Only runs when building on Windows for Windows, where the resource compiler
//! is available.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/app-icon/rewind-os.ico");
    println!("cargo:rerun-if-changed=rewind-os.manifest");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if !(target_windows && cfg!(windows)) {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/rewind-os.ico");
    res.set("ProductName", "Rewind OS");
    res.set("FileDescription", "Rewind OS");
    res.set_manifest_file("rewind-os.manifest");
    if let Err(err) = res.compile() {
        println!("cargo:warning=could not embed Windows resources: {err}");
    }
}
