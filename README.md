# VMM - Virtual Machine with FFI

A workspace containing a concurrency-first virtual machine with Erlang-style scheduling and a comprehensive Foreign Function Interface (FFI) for calling external Rust and Python functions.

## Project Structure

```
vmm/
├── Cargo.toml           # Workspace configuration
├── vmm/                 # Main VM crate
│   ├── src/
│   │   ├── vm.rs        # Erlang-style VM with reduction counting
│   │   ├── value.rs     # Prototype-based object model
│   │   └── ffi_integration.rs # Bridge between VM and FFI
└── vm-ffi/              # Foreign Function Interface crate
    ├── src/
    │   ├── lib.rs       # Core FFI types and registry
    │   ├── rust_ffi.rs  # Rust function implementations
    │   └── bridge.rs    # Value conversion traits
```

## Features

### 🚀 **Erlang-Style Concurrency**
- **Reduction counting**: Each process gets 2000 reductions per scheduling round
- **Preemptive scheduling**: Prevents infinite loops and ensures fairness
- **Process spawning**: `Spawn` instruction for creating new processes
- **Message passing**: `Send`/`Receive` instructions for inter-process communication
- **Process linking**: Bidirectional process monitoring
- **Special VM values**: `__vm:self`, `__vm:pid`, `__vm:processes` for process introspection

### 🧮 **Complete VM Instruction Set**
- **Arithmetic**: Add, Sub, Mul, Div with proper error handling
- **Control Flow**: Jump, JumpIfTrue, JumpIfFalse for branching and loops
- **Function Calls**: Call instruction with support for closures
- **Property Access**: GetProp, SetProp for prototype-based programming
- **Memory Operations**: LoadConst, Move, Return

### 🌐 **Prototype-Based Object Model**
- **Property descriptors**: writable, enumerable, configurable attributes
- **Prototype chains**: Inheritance through prototype relationships
- **Magic methods**: `__getattr__`, `__setattr__` for dynamic behavior
- **Type system**: Objects, Arrays, Functions, Processes as first-class values

### 🔗 **Foreign Function Interface**
- **Rust FFI**: Call Rust functions from VM code
- **Standard Library**: 20+ built-in functions (math, strings, arrays, I/O)
- **Type Conversion**: Seamless VM ↔ FFI value conversion
- **Error Handling**: Proper error propagation and type checking
- **Extensible**: Easy to add new FFI functions

### 🐛 **Developer Experience**
- **Debug output**: Optional detailed VM execution tracing via `--debug` flag
- **CLI tools**: `ionvm` for execution, `iondis` for disassembly, sample generators
- **Actor samples**: Complete working examples including unified actor demo
- **Comprehensive documentation**: Bytecode format, IonPack specification, and examples

## Standard Library Functions

### Math Functions
- `Sqrt`, `Abs`, `Sin`, `Cos`, `Floor`, `Ceil`, `Round`
- `Max`, `Min`

### String Functions  
- `StrLength`, `StrUpper`, `StrLower`, `StrConcat`

### Array Functions
- `ArrayLength`, `ArrayPush`

### I/O Functions
- `Print`, `PrintLn`

### Type Functions
- `IsNumber`, `IsString`, `IsBool`, `IsArray`, `TypeOf`

### Conversion Functions
- `ToString`, `ToNumber`

## Usage Examples

### Basic VM Execution
```rust
use vmm::vm::{IonVM, Instruction};
use vmm::value::{Value, Primitive, Function, FunctionType};
use std::rc::Rc;

// Create VM with optional debug output
let mut vm = IonVM::with_debug(); // Or IonVM::new() for no debug

// Simple function that returns 42
let func = Function {
    name: Some("test".to_string()),
    function_type: FunctionType::Bytecode {
        bytecode: vec![
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
            Instruction::Return(0),
        ]
    }
};

let pid = vm.spawn_process(Rc::new(func), vec![]);
let result = vm.run();
```

### Actor Model Communication
```rust
// Worker function that doubles received numbers
let worker = Function {
    name: Some("worker".to_string()),
    function_type: FunctionType::Bytecode {
        bytecode: vec![
            Instruction::Receive(0),  // Wait for message
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(2.0))),
            Instruction::Mul(2, 0, 1), // Double the input
            Instruction::Return(2),
        ]
    }
};

// Coordinator spawns worker and sends work
let coordinator = Function {
    name: Some("main".to_string()),
    function_type: FunctionType::Bytecode {
        bytecode: vec![
            // Load worker function and spawn process
            Instruction::LoadConst(0, Value::Function(Rc::new(worker))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("__vm:self".to_string()))),
            Instruction::Spawn(2, 0, vec![1]), // spawn(worker, [self])
            
            // Send work to worker
            Instruction::LoadConst(3, Value::Primitive(Primitive::Number(21.0))),
            Instruction::Send(2, 3),
            
            // Receive result
            Instruction::Receive(4),
            Instruction::Return(4), // Should return 42.0
        ]
    }
};
```

### CLI Usage
```bash
# Execute an IonPack file
ionvm run myapp.ionpack

# Execute with debug output
ionvm run --debug myapp.ionpack

# Show package information
ionvm info myapp.ionpack

# Disassemble bytecode
iondis myapp.ionpack Main
```

## Testing

Run all tests:
```bash
cargo test
```

Run specific crate tests:
```bash
cargo test -p vm-ffi     # FFI library tests
cargo test -p vmm        # VM tests
```

Test actor model with debug output:
```bash
cargo run --bin test_actors -- --debug
```

Execute sample IonPacks:
```bash
# Run the unified actor demo
ionvm run --debug unified-actors.ionpack

# Run basic samples
ionvm run hello.ionpack
ionvm run complex.ionpack
```

## Special VM Values

IonVM supports special runtime values with the `__vm:` prefix for process introspection:

- `__vm:self` - Current process reference (for message passing)
- `__vm:pid` - Current process ID as a number  
- `__vm:processes` - Total number of processes in the VM
- `__vm:scheduler_passes` - Number of scheduler passes executed

```assembly
LOAD_CONST r0, '__vm:self'    ; Load current process reference
LOAD_CONST r1, '__vm:pid'     ; Load current process ID
```

For backward compatibility, the bare atom `'self'` is treated as `__vm:self`.

## Debug Output

Enable detailed VM execution tracing:

```rust
// In code
let mut vm = IonVM::with_debug();
// or
vm.set_debug(true);
```

```bash
# Via CLI
ionvm run --debug myapp.ionpack
test_actors --debug
```

Debug output includes:
- Process spawning and scheduling
- Message sending and receiving  
- Instruction execution flow
- Process state changes
- Mailbox operations

## Sample Programs

The project includes several working examples:

- `hello.ionpack` - Simple "Hello, World!" 
- `complex.ionpack` - Object manipulation
- `actors.ionpack` - Basic actor communication
- `unified-actors.ionpack` - Complete actor demo in one file

Generate samples:
```bash
cargo run --bin create_sample unified  # Creates unified-actors.ionpack
```

## Future Enhancements

### Additional Features
- [ ] Pattern matching (`Match` instruction)
- [ ] Hot code reloading
- [x] Debug output control
- [ ] JIT compilation
- [ ] Memory management optimizations
- [ ] Distributed process communication
- [x] Comprehensive actor model examples
- [x] Special VM value system

## Architecture Highlights

### Erlang-Style Reliability
- **Fair scheduling**: No process can monopolize CPU
- **Fault isolation**: Process failures don't crash the VM
- **Predictable latency**: Bounded execution time per scheduling round

### Prototype-Based Flexibility
- **Dynamic objects**: Add/remove properties at runtime
- **Flexible inheritance**: Multiple prototype chains
- **Operator overloading**: Magic methods for custom behavior

### FFI Performance
- **Zero-copy conversions** where possible
- **Efficient function dispatch** through registry
- **Type-safe interfaces** with automatic validation

## Test Coverage

- **All tests passing** across both crates
- **VM Instructions**: All implemented instructions tested
- **Concurrency**: Process spawning, message passing, scheduling  
- **FFI Integration**: Value conversion, function calls, error handling
- **Object Model**: Property access, prototype chains, magic methods
- **Actor Model**: Cross-process communication, deadlock resolution
- **Special Values**: VM introspection and process references
- **CLI Tools**: Execution, disassembly, and debugging workflows

## Recent Improvements

### ✅ Actor Model Fixes (Completed)
- **Deadlock resolution**: Fixed instruction pointer advancement on blocking operations
- **Stack overflow prevention**: Removed recursive debug output  
- **Process communication**: Reliable message passing between actors
- **Scheduler robustness**: Proper process state management

### ✅ Developer Experience (Completed)  
- **Debug output control**: Optional VM tracing via `--debug` flag
- **Special value system**: `__vm:` prefixed runtime values for introspection
- **Unified actor sample**: Complete working example in single file
- **CLI integration**: Debug flags in `ionvm` and `test_actors` tools

### ✅ Documentation (Completed)
- **Comprehensive bytecode specification**: Complete instruction set documentation
- **IonPack format guide**: Module packaging and distribution format
- **Working examples**: Actor model, object manipulation, basic programs
- **CLI usage guides**: Execution, debugging, and disassembly workflows

This VM provides a solid foundation for building a modern, concurrent programming language with excellent interoperability, robust actor semantics, and comprehensive developer tooling.
</edits>
