//! IonVM Disassembler - Convert bytecode to human-readable text format

use std::env;
use std::fs::File;
use std::process;

use vmm::ionpack::{IonPackReader, IonPackError};
use vmm::bytecode_text::function_to_text;
use vmm::bytecode_binary::deserialize_function;

/// CLI error types
#[derive(Debug)]
enum DisError {
    InvalidArgs(String),
    IonPackError(IonPackError),
    IoError(std::io::Error),
    BytecodeError(vmm::bytecode_binary::BytecodeError),
}

impl From<IonPackError> for DisError {
    fn from(err: IonPackError) -> Self {
        DisError::IonPackError(err)
    }
}

impl From<std::io::Error> for DisError {
    fn from(err: std::io::Error) -> Self {
        DisError::IoError(err)
    }
}

impl From<vmm::bytecode_binary::BytecodeError> for DisError {
    fn from(err: vmm::bytecode_binary::BytecodeError) -> Self {
        DisError::BytecodeError(err)
    }
}

impl std::fmt::Display for DisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisError::InvalidArgs(msg) => write!(f, "Invalid arguments: {}", msg),
            DisError::IonPackError(err) => write!(f, "IonPack error: {}", err),
            DisError::IoError(err) => write!(f, "IO error: {}", err),
            DisError::BytecodeError(err) => write!(f, "Bytecode error: {}", err),
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let result = match args[1].as_str() {
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        },
        _ => {
            // Parse file and optional class name
            let file_path = &args[1];
            let class_name = args.get(2);
            
            if file_path.ends_with(".ionpack") {
                disassemble_ionpack(file_path, class_name)
            } else if file_path.ends_with(".ionc") {
                if class_name.is_some() {
                    Err(DisError::InvalidArgs(
                        "Class name not needed for .ionc files".to_string()
                    ))
                } else {
                    disassemble_ionc(file_path)
                }
            } else {
                Err(DisError::InvalidArgs(
                    "File must be .ionpack or .ionc".to_string()
                ))
            }
        }
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        process::exit(1);
    }
}

fn print_usage() {
    println!("IonDis - IonVM Bytecode Disassembler");
    println!();
    println!("USAGE:");
    println!("    iondis <file> [class_name]");
    println!();
    println!("ARGUMENTS:");
    println!("    <file>         .ionpack or .ionc file to disassemble");
    println!("    [class_name]   Class name (required for .ionpack files)");
    println!();
    println!("EXAMPLES:");
    println!("    iondis myapp.ionpack Main       # Disassemble Main class from IonPack");
    println!("    iondis myclass.ionc              # Disassemble raw bytecode file");
    println!("    iondis help                      # Show this help message");
}

fn disassemble_ionpack(file_path: &str, class_name: Option<&String>) -> Result<(), DisError> {
    let class_name = class_name.ok_or_else(|| {
        DisError::InvalidArgs("Class name required for .ionpack files".to_string())
    })?;

    println!("Disassembling class '{}' from IonPack: {}", class_name, file_path);
    println!();

    // Load the IonPack
    let file = File::open(file_path)?;
    let mut reader = IonPackReader::new(file)?;

    // Load all functions from the specified class (supports multi-function format)
    let functions = reader.load_functions(class_name)?;
    
    if functions.is_empty() {
        println!("No functions found in class '{}'", class_name);
        return Ok(());
    }
    
    // Convert each function to text format
    for (i, function) in functions.iter().enumerate() {
        if i > 0 {
            println!(); // Separator between functions
        }
        let text = function_to_text(&function);
        println!("{}", text);
    }

    Ok(())
}

fn disassemble_ionc(file_path: &str) -> Result<(), DisError> {
    println!("Disassembling bytecode file: {}", file_path);
    println!();

    // Read the bytecode file
    let mut file = File::open(file_path)?;
    let function = deserialize_function(&mut file)?;
    
    // Convert to text format
    let text = function_to_text(&function);
    println!("{}", text);

    Ok(())
}
