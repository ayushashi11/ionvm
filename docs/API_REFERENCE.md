# IonVM API Reference

## Table of Contents

- [VM Core API](#vm-core-api)
- [FFI Interface](#ffi-interface)
- [Python Bindings](#python-bindings)
- [IonPack Format](#ionpack-format)

## VM Core API

### IonVM Struct

The main virtual machine implementation providing actor-model execution.

```rust
pub struct IonVM {
    pub processes: HashMap<usize, Rc<RefCell<Process>>>,
    pub run_queue: VecDeque<usize>,
    pub next_pid: usize,
    pub reduction_limit: u32,
    pub timeslice: u32,
    pub scheduler_passes: u64,
    pub ffi_registry: FfiRegistry,
    pub debug: bool,
}
```

#### Methods

##### `new() -> Self`
Creates a new VM instance with default configuration.

```rust
let mut vm = IonVM::new();
```

##### `with_ffi_registry(ffi_registry: FfiRegistry) -> Self`
Creates VM with custom FFI registry.

##### `set_debug(&mut self, debug: bool)`
Enable/disable debug output during execution.

##### `spawn_process(&mut self, function: Rc<Function>, args: Vec<Value>) -> usize`
Spawns a new process with the given function and arguments.

**Returns**: Process ID (PID)

##### `spawn_main_process(&mut self, function: Function) -> Result<Value, String>`
Spawns and executes a main process to completion.

**Returns**: Final return value of the main process

##### `run(&mut self)`
Main scheduler loop - executes processes until completion or deadlock.

### Value Types

#### Core Value Enum

```rust
pub enum Value {
    Primitive(Primitive),
    Tuple(Rc<Vec<Value>>),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<Object>>),
    Function(Rc<RefCell<Function>>),
    Closure(Rc<Closure>),
    Process(Rc<RefCell<Process>>),
}
```

#### Primitive Types

```rust
pub enum Primitive {
    Number(f64),
    Boolean(bool),
    Complex(Complex64),
    String(String),
    Atom(String),
    Unit,
    Undefined,
}
```

### Instruction Set

#### Memory Instructions

- **`LoadConst(reg: usize, value: Value)`** - Load constant into register
- **`Move(dst: usize, src: usize)`** - Move value between registers
- **`ObjectInit(dst: usize, kvs: Vec<(String, ObjectInitArg)>)`** - Create object with properties

#### Arithmetic Instructions

- **`Add(dst: usize, a: usize, b: usize)`** - Addition (numbers, complex, strings)
- **`Sub(dst: usize, a: usize, b: usize)`** - Subtraction
- **`Mul(dst: usize, a: usize, b: usize)`** - Multiplication (includes string repetition)
- **`Div(dst: usize, a: usize, b: usize)`** - Division with zero-check

#### Comparison Instructions

- **`Equal(dst: usize, a: usize, b: usize)`** - Equality comparison
- **`NotEqual(dst: usize, a: usize, b: usize)`** - Inequality comparison
- **`LessThan(dst: usize, a: usize, b: usize)`** - Less than comparison
- **`LessEqual(dst: usize, a: usize, b: usize)`** - Less than or equal
- **`GreaterThan(dst: usize, a: usize, b: usize)`** - Greater than comparison
- **`GreaterEqual(dst: usize, a: usize, b: usize)`** - Greater than or equal

#### Logical Instructions

- **`And(dst: usize, a: usize, b: usize)`** - Logical AND
- **`Or(dst: usize, a: usize, b: usize)`** - Logical OR
- **`Not(dst: usize, src: usize)`** - Logical NOT

#### Control Flow Instructions

- **`Jump(offset: isize)`** - Unconditional jump
- **`JumpIfTrue(cond_reg: usize, offset: isize)`** - Conditional jump if true
- **`JumpIfFalse(cond_reg: usize, offset: isize)`** - Conditional jump if false
- **`Call(dst: usize, func: usize, args: Vec<usize>)`** - Function call
- **`Return(reg: usize)`** - Return from function

#### Actor Instructions

- **`Spawn(dst: usize, func: usize, args: Vec<usize>)`** - Spawn new process
- **`Send(proc: usize, msg: usize)`** - Send message to process
- **`Receive(dst: usize)`** - Blocking receive message
- **`ReceiveWithTimeout(dst: usize, timeout: usize, result: usize)`** - Receive with timeout
- **`Link(proc_id: usize, proc_return_value: usize)`** - Link to process (wait for completion)

#### Object Instructions

- **`GetProp(dst: usize, obj: usize, key: usize)`** - Get object property
- **`SetProp(obj: usize, key: usize, value: usize)`** - Set object property

#### Pattern Matching

- **`Match(src: usize, patterns: Vec<(Pattern, isize)>)`** - Pattern matching with jumps

#### Process Control

- **`Select(dst: usize, pids: Vec<usize>)`** - Wait for first process to complete
- **`SelectWithKill(dst: usize, pids: Vec<usize>)`** - Wait for first, kill others
- **`Yield`** - Explicit yield point (for generators)
- **`Nop`** - No operation

### Object Model

#### Object Structure

```rust
pub struct Object {
    pub properties: HashMap<String, PropertyDescriptor>,
    pub prototype: Option<Rc<RefCell<Object>>>,
    pub magic_methods: HashMap<String, Value>,
    pub type_name: Option<String>,
}
```

#### Property Descriptors

```rust
pub struct PropertyDescriptor {
    pub value: Value,
    pub writable: bool,      // Can property be modified
    pub enumerable: bool,    // Can property be enumerated
    pub configurable: bool,  // Can property descriptor be changed
}
```

#### Object Methods

##### `get_property(&self, key: &str) -> Option<Value>`
Retrieves property value, traversing prototype chain if necessary.

##### `set_property(&mut self, key: &str, value: Value)`
Sets property value, respecting property descriptor flags.

### Function Types

#### Function Structure

```rust
pub struct Function {
    pub name: Option<String>,
    pub arity: usize,
    pub extra_regs: usize,
    pub function_type: FunctionType,
    pub bound_this: Option<Value>,
}

pub enum FunctionType {
    Bytecode { bytecode: Vec<Instruction> },
    Ffi { function_name: String },
}
```

#### Function Methods

##### `new_bytecode(name: Option<String>, arity: usize, extra_regs: usize, bytecode: Vec<Instruction>) -> Self`
Creates a new bytecode function.

##### `new_ffi(name: Option<String>, arity: usize, function_name: String) -> Self`
Creates a new FFI function reference.

##### `total_registers(&self) -> usize`
Returns total number of registers needed (arity + extra_regs).

## FFI Interface

### FfiRegistry

Central registry for external functions.

```rust
pub struct FfiRegistry {
    functions: HashMap<String, Arc<dyn FfiFunction>>,
}
```

#### Methods

##### `new() -> Self`
Creates empty registry.

##### `with_stdlib() -> Self`
Creates registry with standard library functions.

##### `register<F: FfiFunction + 'static>(&mut self, function: F)`
Registers a new FFI function.

##### `call(&self, name: &str, args: Vec<FfiValue>) -> FfiResult`
Calls registered function by name.

### FfiFunction Trait

```rust
pub trait FfiFunction: Send + Sync {
    fn call(&self, args: Vec<FfiValue>) -> FfiResult;
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    fn is_variadic(&self) -> bool { false }
    fn description(&self) -> Option<&str> { None }
}
```

### FfiValue Types

```rust
pub enum FfiValue {
    Number(f64),
    Boolean(bool),
    Atom(String),
    String(String),
    Complex(Complex64),
    Unit,
    Undefined,
    Tuple(Vec<FfiValue>),
    Array(Vec<FfiValue>),
    Object(HashMap<String, FfiValue>),
}
```

### Standard Library Functions

#### Math Functions
- `sin(x)` - Sine function
- `cos(x)` - Cosine function
- `sqrt(x)` - Square root
- `abs(x)` - Absolute value
- `min(a, b)` - Minimum of two values
- `max(a, b)` - Maximum of two values

#### I/O Functions
- `print(value)` - Print value to stdout
- `debug(value)` - Debug print with formatting

## Python Bindings

### Value Creation

```python
from ionvm import Value

# Primitive values
Value.number(42.0)
Value.boolean(True)
Value.string("hello")
Value.atom("symbol")
Value.complex(3+4j)
Value.unit()
Value.undefined()

# Composite values
Value.array([Value.number(1), Value.number(2)])
Value.tuple([Value.number(1), Value.number(2)])
Value.object({
    "x": Value.number(10),
    "y": Value.number(20)
})
```

### Instruction Creation

```python
from ionvm import Instruction

# Memory operations
Instruction.load_const(0, Value.number(42))
Instruction.move(1, 0)

# Arithmetic
Instruction.add(2, 0, 1)
Instruction.sub(2, 0, 1)
Instruction.mul(2, 0, 1)
Instruction.div(2, 0, 1)

# Control flow
Instruction.jump(5)
Instruction.jump_if_true(0, 3)
Instruction.call(0, 1, [2, 3])
Instruction.return_reg(0)

# Actor operations
Instruction.spawn(0, 1, [2, 3])
Instruction.send(0, 1)
Instruction.receive(0)
Instruction.receive_with_timeout(0, 1, 2)
```

### Function Creation

```python
from ionvm import Function, Instruction, Value

function = Function(
    name="add_numbers",
    arity=2,                    # Takes 2 arguments
    extra_regs=1,              # One extra register for result
    instructions=[
        Instruction.add(2, 0, 1),      # r2 = r0 + r1
        Instruction.return_reg(2)       # return r2
    ]
)
```

### IonPack Builder

```python
from ionvm import IonPackBuilder, Function, Instruction, Value

# Create builder
builder = IonPackBuilder("my-program", "1.0.0")
builder.description("A simple IonVM program")
builder.author("Developer Name")

# Set main class and entry point
builder.main_class("Main")
builder.entry_point("main")

# Add functions
main_function = Function(
    name="main",
    arity=0,
    extra_regs=1,
    instructions=[
        Instruction.load_const(0, Value.number(42)),
        Instruction.return_reg(0)
    ]
)

builder.add_class("Main", main_function)

# Build package
with open("program.ionpack", "wb") as f:
    builder.build(f)
```

### Control Flow Builders

```python
from ionvm.control_flow import build_if_else, build_while_then_else

# If-else construction
if_else_instructions = build_if_else(
    condition_reg=0,
    then_instructions=[
        Instruction.load_const(1, Value.string("true branch"))
    ],
    else_instructions=[
        Instruction.load_const(1, Value.string("false branch"))
    ]
)

# While loop construction
while_instructions = build_while_then_else(
    condition_reg=0,
    body_instructions=[
        Instruction.add(0, 0, 1),  # increment counter
    ],
    else_instructions=[
        Instruction.load_const(2, Value.string("loop finished"))
    ]
)
```

## IonPack Format

### Manifest Structure

```python
class Manifest:
    def __init__(self):
        self.name = ""
        self.version = ""
        self.main_class = None
        self.entry_point = None
        self.description = None
        self.author = None
        self.dependencies = []
        self.ffi_libraries = []
        self.exports = []
        self.ionpack_version = "1.0"
```

### Package Structure

```
package.ionpack (ZIP file)
├── META-INF/
│   └── MANIFEST.ion
├── classes/
│   ├── Main.ionc
│   └── Utils.ionc
├── lib/
│   └── native_lib.so
├── resources/
│   └── data.txt
└── src/
    └── main.ion
```

### Reading IonPack Files

```rust
use vmm::{IonPackReader, IonVM};

// Load IonPack
let mut reader = IonPackReader::from_file("program.ionpack")?;
let manifest = reader.read_manifest()?;

// Load main function
let main_class = manifest.main_class.unwrap();
let entry_point = manifest.entry_point.unwrap();
let function = reader.load_class_function(&main_class, &entry_point)?;

// Execute
let mut vm = IonVM::new();
let result = vm.spawn_main_process(function)?;
```

This API reference provides comprehensive coverage of IonVM's interfaces for VM development, FFI integration, and Python-based tooling.
