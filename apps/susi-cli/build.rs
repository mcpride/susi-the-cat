//! Windows only: embeds the app icon and version info (VERSIONINFO) into `susi-cli.exe`.
//!
//! A missing resource compiler (`rc.exe` from the Windows SDK) is only a warning, unless
//! `SUSI_REQUIRE_WINRES` is set (release builds), which turns it into an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/susi.ico");
    println!("cargo:rerun-if-env-changed=SUSI_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/susi.ico")
        .set("ProductName", "Susi the CAT")
        .set("FileDescription", "Susi the CAT command line (SlMake compatible)")
        .set("OriginalFilename", "susi-cli.exe")
        .set("InternalName", "susi-cli");
    if let Err(e) = res.compile() {
        assert!(std::env::var_os("SUSI_REQUIRE_WINRES").is_none(), "embedding Windows resources failed: {e}");
        println!("cargo:warning=susi-cli.exe built without icon/version resources: {e}");
    }
}
