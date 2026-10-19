use std::env;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::process;

use vmm::bytecode::disasm::print_disassembly_with_options;
use vmm::bytecode::serde::read_bytecode;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: iondis [--llvm|-l] <bytecode_file.bin>");
        process::exit(1);
    }

    let mut show_llvm = false;
    let mut filename = "".to_string();

    for arg in &args[1..] {
        if arg == "--llvm" || arg == "-l" {
            show_llvm = true;
        } else if !arg.starts_with('-') {
            filename = arg.clone();
        }
    }

    if filename.is_empty() {
        eprintln!("Usage: iondis [--llvm|-l] <bytecode_file.bin>");
        process::exit(1);
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

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    if let Err(e) = print_disassembly_with_options(&mut handle, &bytecode, show_llvm) {
        eprintln!("Disassembly error: {}", e);
        process::exit(1);
    }
    let _ = handle.flush();
}
