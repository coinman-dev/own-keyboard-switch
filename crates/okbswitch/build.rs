fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/okbswitch.rc");
    println!("cargo:rerun-if-changed=../../images/okbswitch.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace directory");
    let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    let numeric = format!(
        "{},{},{},0",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    let template = std::fs::read_to_string(root.join("packaging/windows/okbswitch.rc"))
        .expect("resource template");
    let icon = root
        .join("images/okbswitch.ico")
        .to_string_lossy()
        .into_owned();
    // Windows canonicalize adds a verbatim prefix which rc.exe does not accept
    // after slash normalization. Keep a regular absolute drive/UNC path.
    let icon = if let Some(unc) = icon.strip_prefix("\\\\?\\UNC\\") {
        format!("//{}", unc.replace('\\', "/"))
    } else {
        icon.strip_prefix("\\\\?\\")
            .unwrap_or(&icon)
            .replace('\\', "/")
    };
    let resource = template
        .replace("@VERSION@", &version)
        .replace("@NUMERIC_VERSION@", &numeric)
        .replace("@ICON@", &icon);
    let file =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("build output directory"))
            .join("okbswitch.rc");
    std::fs::write(&file, resource).expect("expanded resources");
    embed_resource::compile_for(file, ["okbswitch"], embed_resource::NONE)
        .manifest_required()
        .expect("compile the Windows application icon");
}
