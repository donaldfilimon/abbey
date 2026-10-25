//! Review these diagnostic strings when changing the pinned compiler or macros.
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};

fn run(wrapper: &Path, args: &[&str], dir: &Path) -> Output {
    Command::new(wrapper)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn intended_macro_errors_without_sdk_or_panics() {
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let root = vendor.parent().unwrap();
    let wrapper = root.join("tools/cargo.sh");
    let compiler = run(&wrapper, &["--abbey-qualify-toolchain"], root);
    assert!(compiler.status.success());
    assert!(String::from_utf8_lossy(&compiler.stdout).contains("release: 1.99.1"));
    let dir = root.join(format!(".work/gtk/diagnostics-{}", std::process::id()));
    fs::create_dir_all(dir.join("src/bin")).unwrap();
    let mut manifest = fs::read_to_string(vendor.join("compat-tests/Cargo.toml"))
        .unwrap()
        .replace("name = \"gtk-compat-tests\"", "name = \"gtk-compat-ui\"");
    manifest.push_str("\n[workspace]\n[patch.crates-io]\n");
    for (name, version) in [
        ("glib", "0.18.5"),
        ("glib-macros", "0.18.5"),
        ("gtk3-macros", "0.18.2"),
    ] {
        manifest.push_str(&format!(
            "{name} = {{ path = {:?} }}\n",
            vendor.join(format!("{name}-{version}"))
        ));
    }
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    let cases = [
        ("enum_struct", "#[derive(glib_macros::Enum)] struct Wrong; fn main() {}", "#[derive(glib::Enum)] only supports enums"),
        ("enum_name", "#[derive(glib_macros::Enum)] enum Wrong { One } fn main() {}", "#[derive(glib::Enum)] requires #[enum_type(name = \"EnumTypeName\")]"),
        ("template_missing", "#[derive(gtk3_macros::CompositeTemplate)] struct Wrong; fn main() {}", "Missing 'template' attribute: derive(CompositeTemplate) requires #[template(...)]"),
        ("template_child", "#[derive(gtk3_macros::CompositeTemplate)] #[template(string = \"<interface/>\")] struct Wrong { #[template_child(id = 7)] child: () } fn main() {}", "expected string literal"),
    ];
    for (name, source, _) in cases {
        fs::write(dir.join(format!("src/bin/{name}.rs")), source).unwrap();
    }
    fs::copy(vendor.join("Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    let lock = run(&wrapper, &["generate-lockfile", "--offline"], &dir);
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    // Fixture resolution must preserve the reviewed replacement versions.
    let resolved = fs::read_to_string(dir.join("Cargo.lock")).unwrap();
    assert!(!resolved.contains("name = \"proc-macro-error\""));
    assert!(!resolved.contains("name = \"proc-macro-error2\""));
    assert!(resolved.contains("name = \"proc-macro-error3\"\nversion = \"3.1.1\""));
    for (name, _, expected) in cases {
        let output = run(
            &wrapper,
            &[
                "check",
                "--locked",
                "--offline",
                "--bin",
                name,
                "--color",
                "never",
            ],
            &dir,
        );
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        fs::write(dir.join(format!("{name}.stderr")), &output.stderr).unwrap();
        assert!(!output.status.success(), "{name} unexpectedly compiled");
        assert!(diagnostic.contains(expected), "{name}: {diagnostic}");
        assert!(
            diagnostic.contains(&format!("src/bin/{name}.rs")),
            "{name}: {diagnostic}"
        );
        for forbidden in [
            "panicked",
            "unresolved",
            "cannot find",
            "could not find",
            "error[E",
            "failed to run custom build",
        ] {
            assert!(
                !diagnostic.contains(forbidden),
                "{name} failed for wrong reason: {diagnostic}"
            );
        }
        assert_eq!(
            diagnostic
                .lines()
                .filter(|line| line.starts_with("error:"))
                .count(),
            2,
            "unexpected additional diagnostic: {diagnostic}"
        );
    }
}
