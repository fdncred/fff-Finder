//! Embeds the app icon in the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/windows");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The icon is found through the include path, next to the .rc file.
        embed_resource::compile(
            "assets/windows/fff-Finder.rc",
            embed_resource::ParamsIncludeDirs(["assets/windows"]),
        )
        .manifest_optional()
        .unwrap();
    }
}
