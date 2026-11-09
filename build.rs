fn main() {
    // The Windows icon/version resource is pre-built (icon/app.res) so that
    // building does not require the Windows SDK / rc.exe. Cross-platform
    // builds simply skip it.
    println!("cargo:rerun-if-changed=icon/app.res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let res = root.join("icon").join("app.res");
    if res.is_file() {
        println!("cargo:rustc-link-arg={}", res.display());
    } else {
        println!("cargo:warning=icon/app.res missing; executable icon not embedded");
    }
}
