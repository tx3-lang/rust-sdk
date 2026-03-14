use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn resolve_tx3c_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("TX3_TX3C_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    None
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
    let tii_path = PathBuf::from(manifest_dir).join("../examples/transfer.tii");
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

    let mut cmd = if let Some(tx3c_path) = resolve_tx3c_path() {
        Command::new(tx3c_path)
    } else {
        Command::new("tx3c")
    };

    let output = cmd
        .arg("codegen")
        .arg("--tii")
        .arg(&tii_path)
        .arg("--template")
        .arg(&template_dir)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("Failed to execute tx3c. Ensure tx3c is available.");

    if !output.status.success() {
        panic!(
            "tx3c codegen failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert!(output_dir.is_dir(), "Output directory not created");
    assert_file_exists(&output_dir.join("Cargo.toml"));
    assert_file_exists(&output_dir.join("lib.rs"));

    let _ = fs::remove_dir_all(&output_dir);
}
