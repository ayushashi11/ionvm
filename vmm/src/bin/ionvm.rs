//! IonVM CLI - Command line interface for executing IonPack files

use std::env;
use std::fs::File;
use std::process;

use vmm::ionpack::{IonPackReader, IonPackError};
use vmm::vm::IonVM;
use vmm::value::{Value, Primitive, FunctionType};

/// CLI error types
#[derive(Debug)]
enum CliError {
    InvalidArgs(String),
    IonPackError(IonPackError),
    IoError(std::io::Error),
    ExecutionError(String),
}

impl From<IonPackError> for CliError {
    fn from(err: IonPackError) -> Self {
        CliError::IonPackError(err)
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::IoError(err)
    }
}

impl From<String> for CliError {
    fn from(err: String) -> Self {
        CliError::ExecutionError(err)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::InvalidArgs(msg) => write!(f, "Invalid arguments: {}", msg),
            CliError::IonPackError(err) => write!(f, "IonPack error: {}", err),
            CliError::IoError(err) => write!(f, "IO error: {}", err),
            CliError::ExecutionError(msg) => write!(f, "Execution error: {}", msg),
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
        "run" => cmd_run(&args[2..]),
        "info" => cmd_info(&args[2..]),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        },
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            print_usage();
            Err(CliError::InvalidArgs("Unknown command".to_string()))
        }
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        process::exit(1);
    }
}

fn print_usage() {
    println!("IonVM - Concurrency-first Virtual Machine");
    println!();
    println!("USAGE:");
    println!("    ionvm <SUBCOMMAND> [OPTIONS]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    run <ionpack-file> [args...]    Execute an IonPack module");
    println!("    info <ionpack-file>             Show information about an IonPack");
    println!("    help                            Show this help message");
    println!();
    println!("OPTIONS:");
    println!("    --debug, -d                     Enable debug output");
    println!();
    println!("EXAMPLES:");
    println!("    ionvm run hello.ionpack");
    println!("    ionvm run --debug my-app.ionpack");
    println!("    ionvm info my-app.ionpack");
}

fn cmd_run(args: &[String]) -> Result<(), CliError> {
    if args.is_empty() {
        return Err(CliError::InvalidArgs("Missing IonPack file".to_string()));
    }

    // Parse debug flag and ionpack path
    let (debug_enabled, ionpack_path, program_args) = parse_run_args(args)?;

    println!("Loading IonPack: {}", ionpack_path);

    // Load the IonPack
    let file = File::open(ionpack_path)?;
    let mut reader = IonPackReader::new(file)?;

    // Show package info
    let manifest = reader.manifest();
    println!("Executing {} v{}", manifest.name, manifest.version);
    if let Some(ref desc) = manifest.description {
        println!("Description: {}", desc);
    }

    // Setup FFI libraries
    let temp_dir = std::env::temp_dir().join("ionvm-ffi");
    std::fs::create_dir_all(&temp_dir)?;
    let ffi_libs = reader.setup_ffi_libraries(&temp_dir)?;
    
    if !ffi_libs.is_empty() {
        println!("Loaded {} FFI libraries", ffi_libs.len());
    }

    // Get main function directly from the main class
    let functions = reader.load_all_functions()?;
    let main_function = reader.get_main_function()
        .map_err(|e| CliError::ExecutionError(format!("Failed to load main function: {}", e)))?;
    
    println!("Found main function: {:?}", main_function.name);

    // Create VM and execute
    let mut vm = IonVM::new();
    vm.set_debug(debug_enabled);
    
    // Convert program arguments to IonVM values
    let vm_args: Vec<Value> = program_args.iter()
        .map(|arg| Value::Primitive(Primitive::Atom(arg.clone())))
        .collect();

    println!("Starting execution...");
    
    // Execute the main function
    let result = execute_main_function(&mut vm, &main_function, vm_args)?;
    
    println!("Execution completed successfully");
    println!("Result: {:?}", result);

    Ok(())
}

fn cmd_info(args: &[String]) -> Result<(), CliError> {
    if args.is_empty() {
        return Err(CliError::InvalidArgs("Missing IonPack file".to_string()));
    }

    let ionpack_path = &args[0];
    
    // Load the IonPack
    let file = File::open(ionpack_path)?;
    let mut reader = IonPackReader::new(file)?;

    // Display manifest information
    let manifest = reader.manifest();
    println!("IonPack Information");
    println!("==================");
    println!("Name: {}", manifest.name);
    println!("Version: {}", manifest.version);
    println!("IonPack Version: {}", manifest.ionpack_version);
    
    if let Some(ref main_class) = manifest.main_class {
        println!("Main Class: {}", main_class);
    }
    
    if let Some(ref description) = manifest.description {
        println!("Description: {}", description);
    }
    
    if let Some(ref author) = manifest.author {
        println!("Author: {}", author);
    }

    // List dependencies
    if !manifest.dependencies.is_empty() {
        println!("Dependencies:");
        for dep in &manifest.dependencies {
            println!("  - {}", dep);
        }
    }

    // List FFI libraries
    if !manifest.ffi_libraries.is_empty() {
        println!("FFI Libraries:");
        for lib in &manifest.ffi_libraries {
            println!("  - {}", lib);
        }
    }

    // List exports
    if !manifest.exports.is_empty() {
        println!("Exports:");
        for export in &manifest.exports {
            println!("  - {}", export);
        }
    }

    // List classes
    let classes = reader.list_classes()?;
    if !classes.is_empty() {
        println!("Classes ({}):", classes.len());
        for class in classes {
            println!("  - {}", class);
        }
    }

    Ok(())
}

/// Parse run command arguments to extract debug flag, ionpack path, and program args
fn parse_run_args(args: &[String]) -> Result<(bool, &String, &[String]), CliError> {
    let mut debug_enabled = false;
    let mut ionpack_index = 0;
    
    // Check for debug flag at the beginning
    if !args.is_empty() && (args[0] == "--debug" || args[0] == "-d") {
        debug_enabled = true;
        ionpack_index = 1;
    }
    
    // Ensure we have an ionpack file after potential debug flag
    if ionpack_index >= args.len() {
        return Err(CliError::InvalidArgs("Missing IonPack file".to_string()));
    }
    
    let ionpack_path = &args[ionpack_index];
    let program_args = &args[ionpack_index + 1..];
    
    Ok((debug_enabled, ionpack_path, program_args))
}

/// Execute the main function in a VM context
/// This is a simplified version for demonstration
fn execute_main_function(vm: &mut IonVM, function: &vmm::value::Function, _args: Vec<Value>) -> Result<Value, CliError> {
    match &function.function_type {
        FunctionType::Bytecode { bytecode } => {
            println!("Executing bytecode function with {} instructions", bytecode.len());
            
            // For demonstration, we'll create a minimal process and execute
            // In a real implementation, this would use the full VM execution model
            let result = vm.spawn_main_process(function.clone())?;
            
            Ok(result)
        },
        FunctionType::Ffi { function_name } => {
            return Err(CliError::ExecutionError(
                format!("FFI main functions not yet supported: {}", function_name)
            ));
        }
    }
}
