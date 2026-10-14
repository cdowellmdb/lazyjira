use std::process::Command;

#[test]
fn version_flags_print_the_package_version_and_exit() {
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lazyjira"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success(), "{flag}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("lazyjira {}\n", env!("CARGO_PKG_VERSION")),
        );
    }
}
