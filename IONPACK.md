# IonPack Format and CLI Execution Documentation

## IonPack Overview

IonPack is IonVM's module packaging format, similar to Java's JAR files. It's a ZIP-based container that bundles compiled bytecode, FFI libraries, resources, and metadata for distribution and execution.

## IonPack Structure

```
example.ionpack (ZIP file)
├── META-INF/
│   └── MANIFEST.ion          # Package metadata
├── classes/                  # Compiled bytecode classes
│   ├── Main.ionc            # Main class bytecode
│   ├── Utils.ionc           # Utility class bytecode
│   └── ...
├── lib/                     # FFI native libraries
│   ├── math.so              # Linux shared library
│   ├── math.dll             # Windows DLL
│   └── ...
├── resources/               # Static resources
│   ├── config.toml
│   ├── data.json
│   └── ...
└── src/                     # Optional source files
    ├── Main.ion
    ├── Utils.ion
    └── ...
```

## Manifest Format (MANIFEST.ion)

The manifest uses a key-value format similar to Java's MANIFEST.MF:

```
IonPack-Version: 1.0
Name: my-application
Version: 2.1.0
Main-Class: Main
Description: Example IonVM application
Author: Developer Name
Dependencies: std, http-client
FFI-Libraries: math.so, crypto.dll
Exports: Utils, DataProcessor
```

### Manifest Fields

| Field | Required | Description |
|-------|----------|-------------|
| `IonPack-Version` | Yes | IonPack format version (currently 1.0) |
| `Name` | Yes | Package name identifier |
| `Version` | Yes | Package version (semantic versioning recommended) |
| `Main-Class` | No | Entry point class for CLI execution |
| `Description` | No | Human-readable description |
| `Author` | No | Package author/maintainer |
| `Dependencies` | No | Comma-separated list of required packages |
| `FFI-Libraries` | No | Native libraries included in lib/ |
| `Exports` | No | Public classes/modules available to importers |

## CLI Execution Model

### Main Function Resolution

When executing an IonPack via CLI (`ionvm run package.ionpack`), the following resolution occurs:

1. **Main-Class Lookup**: The `Main-Class` field in the manifest specifies which class contains the entry point
2. **Function Loading**: The specified class is loaded from `classes/{Main-Class}.ionc`
3. **Entry Point**: The loaded function is treated as the main entry point (typically with arity 0)
4. **Execution**: The VM creates a new process and executes the main function

### Example CLI Execution Flow

```bash
# Execute an IonPack
ionvm run myapp.ionpack

# With arguments (passed to main function)
ionvm run myapp.ionpack arg1 arg2

# Debug mode
ionvm run --debug myapp.ionpack
```

**Internal Resolution Process:**
1. Load `myapp.ionpack` as ZIP archive
2. Read `META-INF/MANIFEST.ion`
3. Extract `Main-Class: MyApp` 
4. Load `classes/MyApp.ionc` bytecode
5. Deserialize function from bytecode
6. Setup FFI libraries from `lib/`
7. Create VM process and execute main function

## Module and Class System

### Class Files (.ionc)

Each `.ionc` file contains a serialized Function object with:

```rust
struct Function {
    name: Option<String>,        // Function name
    arity: usize,               // Parameter count  
    function_type: FunctionType, // Bytecode or FFI
}
```

### Import/Export System

**Exports (in manifest):**
```
Exports: Calculator, MathUtils, Constants
```

**Imports (in bytecode):**
- IonPack supports dependency resolution via the `Dependencies` field
- Dependencies are other IonPack files that must be available
- The VM's module loader resolves imports at runtime

### Dependency Resolution

```
Dependencies: std@1.0, http-client@2.1, math-lib
```

**Resolution Order:**
1. Check local package cache
2. Look in system-wide package directory
3. Download from package registry (if configured)
4. Load and link dependencies before main execution

## FFI Integration

### FFI Library Management

FFI libraries in `lib/` are extracted to temporary directories during execution:

```rust
// Extract FFI libraries
let temp_dir = std::env::temp_dir().join("ionvm-ffi");
let ffi_libs = reader.setup_ffi_libraries(&temp_dir)?;

// Load libraries into FFI registry
for lib_path in ffi_libs {
    ffi_registry.load_library(&lib_path)?;
}
```

### Platform-Specific Libraries

IonPack can contain multiple platform variants:

```
lib/
├── linux/
│   ├── x86_64/
│   │   └── math.so
│   └── aarch64/
│       └── math.so
├── windows/
│   ├── x64/
│   │   └── math.dll
│   └── x86/
│       └── math.dll
└── macos/
    └── math.dylib
```

The runtime selects appropriate libraries based on the target platform.

## Resource Access

### Runtime Resource Loading

Resources can be accessed at runtime via VM APIs:

```rust
// Load configuration from resources
let config_data = ionpack.read_resource("config.toml")?;
let config: Config = toml::from_slice(&config_data)?;

// Load static data
let lookup_table = ionpack.read_resource("data/lookup.bin")?;
```

### Resource Types

Common resource patterns:
- **Configuration**: `.toml`, `.json`, `.yaml` files
- **Data**: Binary lookup tables, pre-computed data
- **Assets**: Images, audio, text files for applications
- **Templates**: Code generation templates
- **Documentation**: Embedded help files

## IonPack API Usage

### Creating IonPacks

```rust
use ionpack::{IonPackBuilder, Manifest};

let mut builder = IonPackBuilder::new("my-app".to_string(), "1.0.0".to_string())
    .main_class("Main".to_string())
    .description("My IonVM Application".to_string())
    .author("Developer".to_string());

// Add compiled main function
builder.add_class("Main", &main_function)?;

// Add FFI library
builder.add_library("math", "/path/to/libmath.so")?;

// Add resources
builder.add_resource("config.toml", "/path/to/config.toml")?;

// Build package
let mut file = File::create("my-app.ionpack")?;
builder.build(file)?;
```

### Loading and Executing IonPacks

```rust
use ionpack::IonPackReader;
use std::fs::File;

// Load package
let file = File::open("my-app.ionpack")?;
let mut reader = IonPackReader::new(file)?;

// Get package info
let manifest = reader.manifest();
println!("Running {} v{}", manifest.name, manifest.version);

// Setup FFI
let temp_dir = std::env::temp_dir().join("ionvm-ffi");
let ffi_libs = reader.setup_ffi_libraries(&temp_dir)?;

// Load main function
let main_function = reader.get_main_function()?;

// Execute in VM
let mut vm = IonVM::new();
let result = vm.execute_function(&main_function, vec![])?;
```

## CLI Tools Integration

### Main CLI (`ionvm`)
Execute and inspect IonPack files:
```bash
# Execute the main function
ionvm run myapp.ionpack

# Show package information
ionvm info myapp.ionpack
```

### Disassembler (`iondis`)  
Inspect bytecode contents of IonPack classes:
```bash
# Disassemble a specific class
iondis myapp.ionpack MyClass

# Disassemble raw bytecode
iondis myclass.ionc
```

This allows developers to inspect the compiled bytecode for debugging and understanding the execution flow.

## Advanced Features

### Hot-Swapping Modules

IonPacks support runtime module replacement:

```rust
// Replace a module at runtime
vm.unload_module("old-module")?;
let new_module = IonPackReader::new(new_module_file)?;
vm.load_module(new_module)?;
```

### Package Signing and Verification

IonPacks can include digital signatures for security:

```
META-INF/
├── MANIFEST.ion
├── SIGNATURE.ion      # Digital signature
└── CERT.ion          # Certificate chain
```

### Dependency Versions and Conflicts

The dependency resolver handles version conflicts:

```
Dependencies: http-client@>=2.0,<3.0, json@1.5.2
```

**Resolution Strategy:**
- Semantic versioning with ranges
- Conflict detection and resolution
- Dependency graph validation
- Circular dependency detection

## Security Model

### Sandboxing

IonPacks run in isolated environments:
- Process isolation via IonVM's actor model
- Resource access controls
- FFI permission system
- Network access restrictions

### Code Verification

- Bytecode validation during loading
- Function signature verification
- Memory safety through Rust's ownership model
- Type safety at runtime

This packaging system enables IonVM to support complex applications while maintaining security, modularity, and ease of distribution.
