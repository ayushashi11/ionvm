use std::env;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::process;

use vmm::bytecode::serde::read_bytecode;
use vmm::vm::IonVM;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: ionvm [--disasm|-d] [--llvm|-l] [--nvidia|--cuda] [--nvidia-info|--cuda-info] <bytecode_file.bin>");
        process::exit(1);
    }

    let mut is_disasm = false;
    let mut show_llvm = false;
    let mut enable_cuda = false;
    let mut filename = "test_munchausen.bin".to_string();

    for arg in &args[1..] {
        if arg == "--disasm" || arg == "-d" {
            is_disasm = true;
        } else if arg == "--llvm" || arg == "-l" {
            show_llvm = true;
            is_disasm = true;
        } else if arg == "--nvidia-info" || arg == "--cuda-info" {
            if vmm::jit::cuda::is_cuda_available() {
                match vmm::jit::cuda::get_cuda_device_info(0) {
                    Ok(info) => {
                        println!("IonVM NVIDIA CUDA Subsystem:");
                        println!("  Driver: Available (libcuda.so.1)");
                        println!("  Device 0: {}", info.name);
                        println!("  Architecture: {} (Compute Capability {}.{})", info.arch, info.major, info.minor);
                        println!("  VRAM: {} MB", info.total_memory / (1024 * 1024));
                    }
                    Err(e) => {
                        eprintln!("CUDA driver available, but device query failed: {}", e);
                    }
                }
            } else {
                println!("IonVM NVIDIA CUDA Subsystem:");
                println!("  Driver: Not Available (libcuda.so.1 not found)");
            }
            return;
        } else if arg == "--nvidia" || arg == "--cuda" {
            enable_cuda = true;
        } else if !arg.starts_with('-') {
            filename = arg.clone();
        }
    }

    if enable_cuda {
        if vmm::jit::cuda::is_cuda_available() {
            if let Ok(info) = vmm::jit::cuda::get_cuda_device_info(0) {
                eprintln!("[IonVM] NVIDIA CUDA acceleration enabled: {} ({}, {} MB VRAM)", info.name, info.arch, info.total_memory / (1024 * 1024));
            }
        } else {
            eprintln!("[IonVM Warning] --nvidia requested, but NVIDIA CUDA driver (libcuda.so.1) is not available. Falling back to CPU.");
            enable_cuda = false;
        }
    }

    let file = match File::open(&filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let mut reader = BufReader::new(file);
    let bytecode = match read_bytecode(&mut reader) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Failed to parse bytecode: {:?}", e);
            process::exit(1);
        }
    };

    if is_disasm {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        if let Err(e) = vmm::bytecode::disasm::print_disassembly_with_options(&mut handle, &bytecode, show_llvm) {
            eprintln!("Disassembly error: {}", e);
            process::exit(1);
        }
        let _ = handle.flush();
        return;
    }

    let mut vm = match IonVM::new_with_options(bytecode, enable_cuda) {
        Ok(vm) => vm,
        Err(e) => {
            eprintln!("VM init error: {}", e);
            process::exit(1);
        }
    };

    if let Err(e) = vm.run() {
        eprintln!("Execution error: {}", e);
        process::exit(1);
    }

    vmm::jit::llvm_translator::cleanup_jit_engines();
}
