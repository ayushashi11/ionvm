use std::process::Command;

fn ionvm_bin_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop(); // pop test binary name
    if path.ends_with("deps") {
        path.pop(); // pop deps
    }
    path.push("ionvm");
    path
}

#[test]
fn test_cli_no_args() {
    let output = Command::new(ionvm_bin_path())
        .output()
        .expect("failed to execute ionvm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage: ionvm"));
}

#[test]
fn test_cli_nonexistent_file() {
    let output = Command::new(ionvm_bin_path())
        .arg("/nonexistent/file/path.bin")
        .output()
        .expect("failed to execute ionvm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to open file"));
}

#[test]
fn test_cli_corrupted_bytecode() {
    let temp_dir = std::env::temp_dir();
    let corrupt_path = temp_dir.join("test_corrupted.bin");
    std::fs::write(&corrupt_path, b"CORRUPTED_BYTES_HEADER_1234567890").expect("write temp file");

    let output = Command::new(ionvm_bin_path())
        .arg(&corrupt_path)
        .output()
        .expect("failed to execute ionvm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to parse bytecode"));

    let _ = std::fs::remove_file(corrupt_path);
}

#[test]
fn test_cli_truncated_bytecode() {
    let temp_dir = std::env::temp_dir();
    let trunc_path = temp_dir.join("test_truncated.bin");
    // Write just 4 bytes (truncated length)
    std::fs::write(&trunc_path, b"\x01\x00\x00\x00").expect("write temp file");

    let output = Command::new(ionvm_bin_path())
        .arg(&trunc_path)
        .output()
        .expect("failed to execute ionvm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to parse bytecode"));

    let _ = std::fs::remove_file(trunc_path);
}

#[test]
fn test_cli_missing_main() {
    use vmm::bytecode::instruction::Bytecode;
    use vmm::bytecode::serde::write_bytecode;
    use vmm::lang::types::{Function, FunctionType};

    let temp_dir = std::env::temp_dir();
    let bc_path = temp_dir.join("test_no_main.bin");

    let bc = Bytecode {
        functions: vec![Function::new(
            "not_main".to_string(),
            vec![],
            0,
            0,
            FunctionType::Regular,
        )],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut file = std::fs::File::create(&bc_path).expect("create file");
    write_bytecode(&mut file, &bc).expect("write bytecode");

    let output = Command::new(ionvm_bin_path())
        .arg(&bc_path)
        .output()
        .expect("failed to execute ionvm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("main function not found"));

    let _ = std::fs::remove_file(bc_path);
}

#[test]
fn test_cli_disasm_flag_on_corrupt() {
    let temp_dir = std::env::temp_dir();
    let corrupt_path = temp_dir.join("test_disasm_corrupt.bin");
    std::fs::write(&corrupt_path, b"BAD_DATA").expect("write temp file");

    let output = Command::new(ionvm_bin_path())
        .arg("-d")
        .arg(&corrupt_path)
        .output()
        .expect("failed to execute ionvm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to parse bytecode"));

    let _ = std::fs::remove_file(corrupt_path);
}

