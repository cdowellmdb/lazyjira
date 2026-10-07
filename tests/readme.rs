//! README.md's install command pins the release tag, so it must name this version.

#[test]
fn readme_install_tag_matches_the_crate_version() {
    let readme = include_str!("../README.md");
    let tags: Vec<&str> = readme
        .lines()
        .filter(|line| line.contains("cargo install"))
        .filter_map(|line| line.split("--tag ").nth(1))
        .map(|rest| rest.split_whitespace().next().unwrap_or(""))
        .collect();
    let expected = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        tags,
        [expected.as_str()],
        "README.md's `cargo install … --tag`"
    );
}
