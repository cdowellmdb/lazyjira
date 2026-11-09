use std::process::Command;

fn stdout_of(flag: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_lazyjira"))
        .arg(flag)
        // Loading the config fails without HOME, so these flags must be handled before it.
        .env_remove("HOME")
        .output()
        .unwrap();
    assert!(output.status.success(), "{flag}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn version_flags_print_the_package_version_and_exit() {
    for flag in ["--version", "-V"] {
        assert_eq!(
            stdout_of(flag),
            format!("lazyjira {}\n", env!("CARGO_PKG_VERSION")),
        );
    }
}

#[test]
fn help_flags_print_the_flags_and_exit() {
    for flag in ["--help", "-h"] {
        assert_eq!(
            stdout_of(flag),
            "lazyjira\n\
             \x20 --dev, --rebuild   Build from source and run (debug)\n\
             \x20 --dev-release      Build from source and run (release)\n\
             \x20 -V, --version      Show the version\n\
             \x20 -h, --help         Show this help\n",
        );
    }
}
