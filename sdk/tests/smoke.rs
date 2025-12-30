// for each .tii file in the examples directory, try to decode the tii file using the sdk
use std::fs;

use tx3_sdk::tii::Protocol;

#[test]
fn test_tii_files() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    let examples_dir = format!("{manifest_dir}/../examples");

    let tii_files = fs::read_dir(examples_dir).unwrap();

    for file in tii_files {
        let path = file.unwrap().path();

        if path.extension().unwrap() != "tii" {
            continue;
        }

        let tii = fs::read_to_string(path.clone())
            .map_err(|e| eprintln!("Error reading file {}: {}", path.display(), e))
            .unwrap();

        let protocol = Protocol::from_string(tii).unwrap();

        for tx in protocol.txs().keys() {
            let _ = protocol.invoke(tx, None).unwrap();
        }
    }
}
