use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ionvm_root = manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf();

    let libgui_cpp = ionvm_root.join("libgui.cpp");
    let libgui_so = ionvm_root.join("libgui.so");
    let ion_c_abi_h = ionvm_root.join("include").join("ion_c_abi.h");

    let rust_plugin_rs = ionvm_root.join("rust_plugin.rs");
    let librust_plugin_so = ionvm_root.join("librust_plugin.so");

    println!("cargo:rerun-if-changed={}", libgui_cpp.display());
    println!("cargo:rerun-if-changed={}", ion_c_abi_h.display());
    println!("cargo:rerun-if-changed={}", rust_plugin_rs.display());

    // 1. Build C++ GTK plugin (libgui.so)
    if libgui_cpp.exists() {
        let pkg = Command::new("pkg-config")
            .args(["--cflags", "--libs", "gtk+-3.0"])
            .output();

        if let Ok(pkg_out) = pkg {
            if pkg_out.status.success() {
                let flags_str = String::from_utf8_lossy(&pkg_out.stdout);
                let flags: Vec<&str> = flags_str.split_whitespace().collect();

                let cxx = std::env::var("CXX").unwrap_or_else(|_| "g++".to_string());
                let mut cmd = Command::new(cxx);
                cmd.args(["-shared", "-fPIC", "-O2", libgui_cpp.to_str().unwrap()]);
                cmd.args(&flags);
                cmd.args(["-o", libgui_so.to_str().unwrap()]);

                let _ = cmd.status();
            }
        }
    }

    // 2. Build Rust plugin (librust_plugin.rlib and librust_plugin.so)
    if rust_plugin_rs.exists() {
        if let Ok(out_dir_str) = std::env::var("OUT_DIR") {
            let out_dir = PathBuf::from(out_dir_str);
            // OUT_DIR is target/debug/build/vmm-<hash>/out
            // target/debug is out_dir.ancestors().nth(3)
            if let Some(target_dir) = out_dir.ancestors().nth(3) {
                let deps_dir = target_dir.join("deps");
                if deps_dir.exists() {
                    // Find newest libvm_ffi-*.rlib
                    let mut best_rlib: Option<(PathBuf, std::time::SystemTime)> = None;
                    if let Ok(entries) = std::fs::read_dir(&deps_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                if name.starts_with("libvm_ffi") && name.ends_with(".rlib") {
                                    let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                    if best_rlib.as_ref().map_or(true, |(_, best_time)| mtime > *best_time) {
                                        best_rlib = Some((path, mtime));
                                    }
                                }
                            }
                        }
                    }

                    if let Some((vm_ffi_rlib, _)) = best_rlib {
                        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
                        let status = Command::new(rustc)
                            .args([
                                "--edition", "2024",
                                "--crate-type", "rlib,cdylib",
                                rust_plugin_rs.to_str().unwrap(),
                                "-L", deps_dir.to_str().unwrap(),
                                "--extern", &format!("vm_ffi={}", vm_ffi_rlib.to_str().unwrap()),
                                "--out-dir", ionvm_root.to_str().unwrap(),
                            ])
                            .status();

                        if let Ok(s) = status {
                            if s.success() {
                                // Also copy to /tmp for any external references
                                let _ = std::fs::copy(&librust_plugin_so, "/tmp/librust_plugin.so");
                            }
                        }
                    }
                }
            }
        }
    }
}
