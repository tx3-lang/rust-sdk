use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn tx3c_command() -> Command {
    if let Ok(path) = env::var("TX3_TX3C_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Command::new(path);
        }
    }
    Command::new("tx3c")
}

fn unique_output_dir() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be available")
        .as_nanos();
    env::temp_dir().join(format!("tx3c_codegen_test_{now}"))
}

fn assert_file_exists(path: &Path) {
    assert!(path.is_file(), "Expected file to exist: {}", path.display());
}

#[test]
fn test_tx3c_codegen_client_lib_template() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let tii_path = PathBuf::from(manifest_dir).join("tests/fixtures/transfer.tii");
    let template_dir = PathBuf::from(manifest_dir).join("../.trix/client-lib");

    assert!(
        tii_path.is_file(),
        "Missing TII file: {}",
        tii_path.display()
    );
    assert!(
        template_dir.is_dir(),
        "Missing template directory: {}",
        template_dir.display()
    );

    let output_dir = unique_output_dir();

    let output = tx3c_command()
        .arg("codegen")
        .arg("--tii")
        .arg(&tii_path)
        .arg("--template")
        .arg(&template_dir)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("Failed to execute tx3c. Set TX3_TX3C_PATH or install tx3c.");

    if !output.status.success() {
        panic!(
            "tx3c codegen failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert!(output_dir.is_dir(), "Output directory not created");
    let cargo_toml = output_dir.join("Cargo.toml");
    assert_file_exists(&cargo_toml);
    assert_file_exists(&output_dir.join("lib.rs"));

    // Point the generated crate at this repo's SDK so the check exercises the
    // code under test rather than a published release.
    let mut manifest = fs::read_to_string(&cargo_toml).expect("read generated Cargo.toml");
    manifest.push_str(&format!(
        "\n[patch.crates-io]\ntx3-sdk = {{ path = \"{manifest_dir}\" }}\n"
    ));
    fs::write(&cargo_toml, manifest).expect("write patched Cargo.toml");

    // A successful render that produces uncompilable bindings is a failure.
    let check = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--manifest-path")
        .arg(&cargo_toml)
        .output()
        .expect("Failed to execute cargo");

    let check_ok = check.status.success();
    let _ = fs::remove_dir_all(&output_dir);

    if !check_ok {
        panic!(
            "cargo check on generated bindings failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr)
        );
    }
}
