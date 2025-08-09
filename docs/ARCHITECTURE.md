# IonVM Architecture Documentation

## Overview

IonVM is a research virtual machine implementing the actor model of computation with support for prototype-based objects, message passing, and preemptive scheduling. The system consists of three main components:

1. **VM Core** (`vmm/`) - Rust-based virtual machine implementation
2. **FFI Interface** (`vm-ffi/`) - Foreign Function Interface for native library integration  
3. **Python Bindings** (`python-ionvm/`) - Python library for bytecode generation and IonPack creation

## VM Core Architecture (`vmm/`)

### Core Components

#### Virtual Machine (`vm.rs`)
The heart of IonVM implementing Erlang-style actor scheduling:

- **Preemptive Scheduler**: Round-robin scheduling with configurable timeslice (default: 3 instructions)
- **Process Management**: Lightweight processes with individual mailboxes and state
- **Reduction Counting**: Budget-based execution to ensure fair scheduling
- **Message Passing**: Asynchronous message delivery with blocking/timeout receives

#### Value System (`value.rs`)
Prototype-based object model supporting:

- **Primitive Types**: Numbers, booleans, strings, atoms, complex numbers
- **Composite Types**: Tuples, arrays, objects with property descriptors
- **Functions**: First-class functions with closures and FFI integration
- **Processes**: Process references for actor communication

```rust
// Core value types
pub enum Value {
    Primitive(Primitive),
    Tuple(Rc<Vec<Value>>),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<Object>>),
    Function(Rc<RefCell<Function>>),
    Closure(Rc<Closure>),
    Process(Rc<RefCell<Process>>),
}

// Object with prototype chain and property descriptors
pub struct Object {
    pub properties: HashMap<String, PropertyDescriptor>,
    pub prototype: Option<Rc<RefCell<Object>>>,
    pub magic_methods: HashMap<String, Value>,
    pub type_name: Option<String>,
}
```

#### Instruction Set
Register-based VM with comprehensive instruction set:

**Memory Operations**:
- `LoadConst(reg, value)` - Load constant into register
- `Move(dst, src)` - Move value between registers
- `ObjectInit(dst, kvs)` - Create object with property descriptors

**Arithmetic Operations**:
- `Add/Sub/Mul/Div(dst, a, b)` - Basic arithmetic with complex number support
- String multiplication for repetition

**Actor Operations**:
- `Spawn(dst, func, args)` - Create new process
- `Send(proc, msg)` - Send message to process
- `Receive(dst)` - Blocking receive
- `ReceiveWithTimeout(dst, timeout, result)` - Receive with timeout

**Control Flow**:
- `Jump/JumpIfTrue/JumpIfFalse` - Conditional and unconditional jumps
- `Call(dst, func, args)` - Function calls with return value
- `Return(reg)` - Return from function

**Pattern Matching**:
- `Match(src, patterns)` - Pattern matching with jump offsets

#### IonPack Format (`ionpack.rs`)
ZIP-based packaging system for distributing IonVM programs:

```
ionpack-file.ionpack (ZIP archive)
├── META-INF/
│   └── MANIFEST.ion          # Package metadata
├── classes/
│   ├── Main.ionc            # Compiled bytecode
│   └── Module.ionc
├── lib/                     # Native FFI libraries
├── resources/               # Static resources
└── src/                     # Optional source files
```

### Process Model

#### Process Lifecycle
```
Runnable → Executing → [BudgetExhausted/Blocked/Exited]
    ↑                              ↓
    └── [Message Received] ←── WaitingForMessage
```

#### Timeout Handling
- **Timeout Tracking**: System tracks pending timeout operations
- **Frame-Aware**: Timeouts associated with specific call frames
- **Automatic Cleanup**: Expired timeouts handled during scheduler passes

```rust
struct TimeoutInfo {
    pid: usize,
    dst_reg: usize,
    result_reg: usize,
    expiry_ms: u64,
    frame_index: usize,
}
```

## FFI Interface (`vm-ffi/`)

### Purpose
Provides bridge between VM and external native functions, enabling:
- Standard library implementation
- Performance-critical operations
- System integration

### Architecture

#### Value Conversion (`bridge.rs`)
Bidirectional conversion between VM values and FFI values:

```rust
pub enum FfiValue {
    Number(f64),
    Boolean(bool),
    String(String),
    Complex(Complex64),
    Array(Vec<FfiValue>),
    Object(HashMap<String, FfiValue>),
    // ...
}
```

#### Function Registry (`lib.rs`)
Dynamic registration and invocation of FFI functions:

```rust
pub trait FfiFunction: Send + Sync {
    fn call(&self, args: Vec<FfiValue>) -> FfiResult;
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    fn description(&self) -> Option<&str>;
}
```

#### Standard Library (`stdlib/`)
Built-in functions for common operations:
- Math functions (sin, cos, sqrt, etc.)
- I/O operations (print, debug output)
- String manipulation
- Array operations

## Python Bindings (`python-ionvm/`)

### Purpose
Python library for:
- Generating IonVM bytecode programmatically
- Creating and manipulating IonPack files
- Building development tools and compilers

### Components

#### Value Creation (`value.py`)
Python API for creating VM values:

```python
class Value:
    @classmethod
    def number(cls, n: float) -> 'Value':
        """Create a number value."""
        
    @classmethod
    def object(cls, properties: Dict[str, 'Value'], 
               writable: Optional[Dict[str, bool]] = None) -> 'Value':
        """Create an object value with property descriptors."""
```

#### Instruction Generation (`instruction.py`)
Type-safe instruction creation:

```python
class Instruction:
    @classmethod
    def add(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Add values in registers a and b, store result in dst."""
        
    @classmethod
    def spawn(cls, dst: float, func: float, args: List[float]) -> 'Instruction':
        """Spawn new process with function and arguments."""
```

#### IonPack Builder (`ionpack.py`)
High-level API for package creation:

```python
class IonPackBuilder:
    def __init__(self, name: str, version: str):
        """Create new IonPack builder."""
        
    def add_class(self, name: str, function: Function):
        """Add a class (function) to the package."""
        
    def build(self, output_stream):
        """Build and write IonPack to stream."""
```

## Key Design Principles

### Actor Model Implementation
- **Isolation**: Each process has separate memory space
- **Asynchronous Communication**: Message passing without shared state
- **Fair Scheduling**: Preemptive scheduler prevents process starvation
- **Fault Tolerance**: Process linking and supervision (partial implementation)

### Prototype-Based Objects
- **Dynamic Properties**: Properties can be added/removed at runtime
- **Prototype Chain**: Inheritance through prototype references
- **Property Descriptors**: Control over property behavior (writable, enumerable, configurable)
- **Magic Methods**: Support for operator overloading (planned)

### Performance Considerations
- **Register-Based VM**: Fewer instructions than stack-based approach
- **Reference Counting**: Rust's Rc/RefCell for memory management
- **Reduction Counting**: Prevents infinite loops and ensures responsiveness
- **Optimized Message Queues**: VecDeque for O(1) message operations

### Extensibility
- **FFI System**: Easy integration of native functions
- **Modular Design**: Clean separation between VM, FFI, and bindings
- **IonPack Format**: Standardized packaging for distribution
- **Python Integration**: Tools for language development and analysis

## Development Workflow

### Building and Testing
```bash
# Build VM core
cargo build -p vmm

# Build FFI
cargo build -p vm-ffi  

# Build Python bindings
cd python-ionvm && pip install -e .

# Run tests
cargo test
cd python-ionvm && python -m pytest
```

### Creating IonPack Files
```python
from ionvm import Function, Instruction, Value, IonPackBuilder

# Create function
function = Function(
    name="main",
    arity=0,
    extra_regs=1,
    instructions=[
        Instruction.load_const(0, Value.number(42)),
        Instruction.return_reg(0)
    ]
)

# Package
builder = IonPackBuilder("hello-world", "1.0.0")
builder.add_class("Main", function)
builder.main_class("Main")
builder.entry_point("main")

with open("hello.ionpack", "wb") as f:
    builder.build(f)
```

### Running Programs
```bash
# Run IonPack file
./target/release/ionvm hello.ionpack

# Disassemble bytecode
./target/release/iondis hello.ionpack

# Debug execution
./target/release/ionvm --debug hello.ionpack
```

This architecture provides a solid foundation for experimenting with actor-model concurrency, dynamic languages, and virtual machine design while maintaining clean abstractions and extensibility.
