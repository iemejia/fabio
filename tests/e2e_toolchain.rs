//! Hermetic consistency checks for the declared minimum Rust version.

#[test]
fn msrv_documentation_and_devcontainer_match_the_package_metadata() {
    let msrv = env!("CARGO_PKG_RUST_VERSION");
    for (file, contents, expected) in [
        (
            "README.md",
            include_str!("../README.md"),
            format!("requires Rust {msrv}+"),
        ),
        (
            "AGENTS.md",
            include_str!("../AGENTS.md"),
            format!("rust-version {msrv}"),
        ),
        (
            "AGENTS.md",
            include_str!("../AGENTS.md"),
            format!("currently {msrv}"),
        ),
        (
            ".github/copilot-instructions.md",
            include_str!("../.github/copilot-instructions.md"),
            format!("MSRV {msrv}"),
        ),
        (
            ".github/copilot-instructions.md",
            include_str!("../.github/copilot-instructions.md"),
            format!("rust-version = \"{msrv}\""),
        ),
        (
            ".devcontainer/Dockerfile",
            include_str!("../.devcontainer/Dockerfile"),
            format!("ARG RUST_MSRV={msrv}"),
        ),
    ] {
        assert!(
            contents.contains(&expected),
            "{file} must declare the package MSRV: {expected}"
        );
    }
    assert!(
        include_str!("../.github/workflows/sync-rust-release.yml")
            .contains(".devcontainer/Dockerfile"),
        "Automated MSRV updates must refresh the devcontainer cache/version guard too"
    );
}
