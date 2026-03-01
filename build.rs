fn main() {
    println!("cargo:rerun-if-env-changed=VIBEMATE_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_TYPE");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");

    let version = std::env::var("VIBEMATE_BUILD_VERSION")
        .ok()
        .or_else(version_from_github_tag)
        .unwrap_or_else(|| {
            std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION should be set by Cargo")
        });

    println!("cargo:rustc-env=VIBEMATE_VERSION={version}");
}

fn version_from_github_tag() -> Option<String> {
    let ref_type = std::env::var("GITHUB_REF_TYPE").ok()?;
    if ref_type != "tag" {
        return None;
    }

    let ref_name = std::env::var("GITHUB_REF_NAME").ok()?;
    ref_name.strip_prefix('v').map(ToOwned::to_owned)
}
