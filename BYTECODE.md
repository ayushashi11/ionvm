# IonVM Bytecode Format Specification

## Overview

IonVM uses a custom bytecode format designed for a concurrency-first, prototype-based virtual machine. The bytecode supports lightweight processes, pattern matching, FFI calls, and dynamic object manipulation.

## Table of Contents

1. [Instruction Set Architecture](#instruction-set-architecture)
2. [Value Types](#value-types)
3. [Core Instructions](#core-instructions)
4. [Binary Format](#binary-format)
5. [Text Assembly Format](#text-assembly-format)
6. [Function Format](#function-format)
7. [Process Model](#process-model)
8. [Error Handling](#error-handling)
9. [Optimization Notes](#optimization-notes)
10. [Examples](#examples)

## Instruction Set Architecture

### Register-Based VM
- IonVM uses a register-based architecture (similar to Dalvik/ART)
- Each function explicitly specifies how many registers it needs
- Registers r0 to r{arity-1} hold function arguments
- Additional registers for calculations are allocated based on the function's `extra_regs` field
- Registers are dynamically typed and can hold any `Value`

### Value Types
```rust
enum Value {
    Primitive(Primitive),      // Basic types
    Tuple(Rc<Vec<Value>>),     // Immutable tuples
    Array(Rc<RefCell<Vec<Value>>>), // Mutable arrays
    Object(Rc<RefCell<Object>>),    // Prototype-based objects
    TaggedEnum(Rc<TaggedEnum>),     // Sum types
    Function(Rc<Function>),         // First-class functions
    Closure(Rc<Closure>),          // Closures with captured env
    Process(Rc<RefCell<Process>>), // Process references
}

enum Primitive {
    Number(f64),        // IEEE 754 double
    Boolean(bool),      // true/false
    Atom(String),       // Interned strings/symbols
    Unit,              // Void/null equivalent
    Undefined,         // Uninitialized/error state
}
```

## Core Instructions

### Memory Operations
```assembly
LOAD_CONST reg, value     ; Load constant into register
MOVE       dst, src       ; Copy value between registers
```

**Example:**
```assembly
LOAD_CONST r0, 42.0       ; r0 = 42.0
LOAD_CONST r1, :hello     ; r1 = atom "hello"
MOVE       r2, r0         ; r2 = r0
```

### Special VM Values

IonVM supports special `__vm:` prefixed values that are resolved at runtime:

```assembly
LOAD_CONST r0, '__vm:self'             ; Load current process reference
LOAD_CONST r1, '__vm:pid'              ; Load current process ID (as number)
LOAD_CONST r2, '__vm:processes'        ; Load total process count
LOAD_CONST r3, '__vm:scheduler_passes' ; Load scheduler pass count
```

**Available __vm: Values:**
- `__vm:self` - Current process reference (for message passing)
- `__vm:pid` - Current process ID as a number
- `__vm:processes` - Total number of processes in the VM
- `__vm:scheduler_passes` - Number of scheduler passes executed

**Legacy Support:**
For backward compatibility, the bare atom `'self'` is treated as `__vm:self`:
```assembly
LOAD_CONST r0, 'self'     ; Equivalent to '__vm:self'
```

**Usage in Actor Model:**
Special values are essential for process communication:
```assembly
; Get reference to current process
LOAD_CONST r0, '__vm:self'
; Pass self-reference to spawned worker
SPAWN r1, worker_func, [r0]
```

### Arithmetic Operations
```assembly
ADD dst, a, b    ; dst = a + b
SUB dst, a, b    ; dst = a - b
MUL dst, a, b    ; dst = a * b
DIV dst, a, b    ; dst = a / b (returns Undefined on div by zero)
```

**Example:**
```assembly
LOAD_CONST r0, 10.0
LOAD_CONST r1, 3.0
ADD        r2, r0, r1     ; r2 = 13.0
MUL        r3, r2, r1     ; r3 = 39.0
```

### Object Operations
```assembly
GET_PROP dst, obj, key    ; dst = obj[key] (follows prototype chain)
SET_PROP obj, key, value  ; obj[key] = value
```

**Example:**
```assembly
LOAD_CONST r0, {object}       ; Load object reference
LOAD_CONST r1, :name          ; Property key
LOAD_CONST r2, "Alice"        ; Property value
SET_PROP   r0, r1, r2         ; obj.name = "Alice"
GET_PROP   r3, r0, r1         ; r3 = obj.name
```

### Control Flow
```assembly
RETURN    reg              ; Return value in register
JUMP      offset           ; Unconditional jump (relative)
JUMP_IF_TRUE  cond, offset ; Jump if register is truthy
JUMP_IF_FALSE cond, offset ; Jump if register is falsy
```

**Example:**
```assembly
LOAD_CONST r0, true
JUMP_IF_TRUE r0, 3         ; Skip next 2 instructions
LOAD_CONST r1, 0           ; This won't execute
RETURN r1
LOAD_CONST r1, 42          ; This will execute
RETURN r1
```

### Function Calls
```assembly
CALL dst, func, [args...]  ; Call function with arguments
```

**Example:**
```assembly
LOAD_CONST r0, function:add   ; Load function reference
LOAD_CONST r1, 10.0          ; First argument
LOAD_CONST r2, 32.0          ; Second argument
CALL       r3, r0, [r1, r2]  ; r3 = add(10.0, 32.0)
```

### Process Operations (Actor Model)
```assembly
SPAWN dst, func, [args...]  ; Create new process
SEND  proc, msg            ; Send message to process
RECEIVE dst                ; Wait for message (blocks if none)
LINK  proc                 ; Create bidirectional link
YIELD                      ; Voluntary preemption point
```

**Example:**
```assembly
; Spawn a worker process
LOAD_CONST r0, function:worker
LOAD_CONST r1, 100
SPAWN      r2, r0, [r1]    ; r2 = new process PID

; Send it a message
LOAD_CONST r3, :work_data
SEND       r2, r3

; Wait for response
RECEIVE    r4              ; Blocks until message arrives
```

### Pattern Matching
```assembly
MATCH src, [(pattern, offset), ...]  ; Pattern match with jump table
```

**Patterns:**
- `Value(v)` - Exact value match
- `Wildcard` - Matches anything (_)
- `Tuple([patterns...])` - Destructure tuples
- `Array([patterns...])` - Destructure arrays
- `TaggedEnum(tag, pattern)` - Match tagged unions

**Example:**
```assembly
LOAD_CONST r0, (ok, 42)           ; Tuple value
MATCH r0, [
    ((error, _), +5),             ; Jump +5 if error tuple
    ((ok, _), +2),                ; Jump +2 if ok tuple
    (_, +8)                       ; Default case
]
; ok case
LOAD_CONST r1, :success
RETURN r1
; error case
LOAD_CONST r1, :failure
RETURN r1
```

### Utility
```assembly
NOP    ; No operation
```

## Binary Format

### File Structure
```
[8 bytes]  Magic: "IONBC\x01\x00\x00"
[4 bytes]  Version: 1 (little-endian u32)
[4 bytes]  Instruction count (little-endian u32)
[variable] Instructions...
```

### Instruction Encoding
Each instruction starts with a 1-byte opcode:

| Opcode | Instruction    | Format |
|--------|---------------|---------|
| 0x01   | LOAD_CONST    | opcode + reg(u32) + value |
| 0x02   | MOVE          | opcode + dst(u32) + src(u32) |
| 0x03   | ADD           | opcode + dst(u32) + a(u32) + b(u32) |
| 0x04   | SUB           | opcode + dst(u32) + a(u32) + b(u32) |
| 0x05   | MUL           | opcode + dst(u32) + a(u32) + b(u32) |
| 0x06   | DIV           | opcode + dst(u32) + a(u32) + b(u32) |
| 0x07   | GET_PROP      | opcode + dst(u32) + obj(u32) + key(u32) |
| 0x08   | SET_PROP      | opcode + obj(u32) + key(u32) + val(u32) |
| 0x09   | CALL          | opcode + dst(u32) + func(u32) + argc(u32) + args... |
| 0x0A   | RETURN        | opcode + reg(u32) |
| 0x0B   | JUMP          | opcode + offset(i32) |
| 0x0C   | JUMP_IF_TRUE  | opcode + cond(u32) + offset(i32) |
| 0x0D   | JUMP_IF_FALSE | opcode + cond(u32) + offset(i32) |
| 0x0E   | SPAWN         | opcode + dst(u32) + func(u32) + argc(u32) + args... |
| 0x0F   | SEND          | opcode + proc(u32) + msg(u32) |
| 0x10   | RECEIVE       | opcode + dst(u32) |
| 0x11   | LINK          | opcode + proc(u32) |
| 0x12   | MATCH         | opcode + src(u32) + count(u32) + patterns... |
| 0x13   | YIELD         | opcode |
| 0x14   | NOP           | opcode |

### Value Encoding
Values are encoded with a 1-byte type tag:

| Tag  | Type      | Format |
|------|-----------|---------|
| 0x01 | Number    | tag + f64(8 bytes) |
| 0x02 | Boolean   | tag + u8 (0=false, 1=true) |
| 0x03 | Atom      | tag + length(u32) + utf8_bytes |
| 0x04 | Unit      | tag |
| 0x05 | Undefined | tag |
| 0x06 | Array     | tag + length(u32) + values... |
| 0x07 | Object    | tag + prop_count(u32) + (key + value + flags)... |
| 0x08 | Function  | tag + name_length(u32) + name |
| 0x09 | Tuple     | tag + length(u32) + values... |

### Pattern Encoding
Patterns use these tags:

| Tag  | Pattern     | Format |
|------|-------------|---------|
| 0x01 | Value       | tag + value |
| 0x02 | Wildcard    | tag |
| 0x03 | Tuple       | tag + length(u32) + patterns... |
| 0x04 | Array       | tag + length(u32) + patterns... |
| 0x05 | TaggedEnum  | tag + tag_name + pattern |

## Text Assembly Format

### Syntax
```assembly
.bytecode                    ; Start directive
; Function: name (arity: N, extra_regs: M)  ; Function header comment
   0: LOAD_CONST r0, 42.0   ; Line numbers are optional
   1: LOAD_CONST r1, true   ; Instructions are case-insensitive
   2: ADD r2, r0, r1        ; Registers use 'r' prefix
   3: RETURN r2             ; Comments start with ';'
.end                        ; End directive
```

### Function Definition Syntax
All functions must explicitly declare their register requirements:

```rust
// Function constructor requires arity and extra_regs
Function::new_bytecode(
    name: Option<String>,    // Function name (optional)
    arity: usize,           // Number of arguments (required)
    extra_regs: usize,      // Additional registers needed (required)
    bytecode: Vec<Instruction> // Function bytecode (required)
)
```

**Examples:**
```rust
// Simple identity function: takes 1 arg, needs no extra registers
let identity = Function::new_bytecode(
    Some("identity".to_string()),
    1,  // arity: r0 = input
    0,  // extra_regs: no additional registers needed
    vec![Instruction::Return(0)]
);

// Math function: takes 2 args, needs 3 extra registers for calculations
let complex_math = Function::new_bytecode(
    Some("complex".to_string()),
    2,  // arity: r0 = a, r1 = b
    3,  // extra_regs: r2, r3, r4 for intermediate calculations
    vec![
        Instruction::Add(2, 0, 1),     // r2 = a + b
        Instruction::LoadConst(3, Value::Primitive(Primitive::Number(2.0))),
        Instruction::Mul(4, 2, 3),     // r4 = (a + b) * 2
        Instruction::Return(4),
    ]
);
```

### Value Literals
```assembly
42.0                ; Number
true, false         ; Boolean
:atom_name          ; Atom (symbol)
"string"            ; String (converted to atom)
'string'            ; Alternative string syntax
unit                ; Unit value
undefined           ; Undefined value
[1, 2, 3]          ; Array (limited parsing)
{object}           ; Object placeholder
(1, 2)             ; Tuple (limited parsing)
function:name      ; Function reference
```

### Register Syntax
```assembly
r0, r1, r2, ..., r{n}    ; Register numbers based on function needs
                         ; r0 to r{arity-1}: function arguments
                         ; r{arity} to r{arity+extra_regs-1}: local registers
```

### Jump Targets
```assembly
JUMP +5             ; Relative forward jump
JUMP -3             ; Relative backward jump
```

## Function Format

### Function Metadata
```rust
struct Function {
    name: Option<String>,           // Function name
    arity: usize,                  // Number of parameters
    extra_regs: usize,             // Additional registers beyond arguments
    function_type: FunctionType,   // Bytecode or FFI
}

enum FunctionType {
    Bytecode { bytecode: Vec<Instruction> },
    Ffi { function_name: String },
}
```

### Register Allocation
Functions specify exactly how many registers they need:
- **Arguments**: Registers r0 to r{arity-1} hold function arguments
- **Extra Registers**: Registers r{arity} to r{arity+extra_regs-1} for calculations
- **Total Registers**: arity + extra_regs (minimum 16 for compatibility)

**Example:**
```rust
// Function with arity=2, extra_regs=3 uses registers r0-r4
let func = Function::new_bytecode(
    Some("math".to_string()),
    2,  // arity: r0=first arg, r1=second arg
    3,  // extra_regs: r2, r3, r4 available for calculations
    vec![
        Instruction::Add(2, 0, 1),     // r2 = r0 + r1
        Instruction::LoadConst(3, Value::Primitive(Primitive::Number(2.0))),
        Instruction::Mul(4, 2, 3),     // r4 = r2 * 2
        Instruction::Return(4),
    ]
);
```

### Function Serialization
```
[1 byte]   Has name flag (0/1)
[variable] Name (if has_name=1): length(u32) + utf8_bytes
[4 bytes]  Arity (u32)
[4 bytes]  Extra registers (u32)
[1 byte]   Function type (0=Bytecode, 1=FFI)

If Bytecode:
[4 bytes]  Instruction count (u32)
[variable] Instructions...

If FFI:
[variable] FFI function name: length(u32) + utf8_bytes
```

## Process Model

### Process Structure
```rust
struct Process {
    pid: usize,                    // Process identifier
    frames: Vec<Frame>,            // Call stack
    mailbox: Vec<Value>,           // Message queue
    links: Vec<usize>,             // Linked processes
    alive: bool,                   // Process state
    status: ProcessStatus,         // Current status
    reductions: u32,               // Instruction budget
}

struct Frame {
    registers: Vec<Value>,         // Register file (arity + extra_regs, min 16)
    stack: Vec<Value>,             // Local stack
    ip: usize,                     // Instruction pointer
    function: Rc<Function>,        // Current function
    return_value: Option<Value>,   // Return value slot
    caller_return_reg: Option<usize>, // Where to store return
}
```

### Scheduling
- **Preemptive**: Processes get a reduction budget (default: 2000 instructions)
- **Fair**: Round-robin scheduling with process queues
- **Message-driven**: Blocked processes wake up when messages arrive
- **Cooperative**: `YIELD` instruction for voluntary preemption

## Error Handling

### Runtime Errors
- **Division by zero**: Returns `Undefined` value
- **Invalid property access**: Returns `Undefined`
- **Type mismatches**: Operations return `Undefined`
- **Process crashes**: Linked processes get exit signals

### Compilation Errors
- **Invalid opcodes**: Rejected during deserialization
- **Malformed instructions**: Detected by binary format validation
- **Version mismatches**: Handled by format version checks

## Optimization Notes

### Performance Characteristics
- **Register allocation**: Explicit allocation based on function requirements
- **Instruction dispatch**: Direct matching in Rust (very fast)
- **Memory management**: Rust's Rc/RefCell for shared ownership
- **Process switching**: Lightweight (just context save/restore)

### Design Decisions
- **Register-based**: Fewer instructions than stack-based VMs
- **Explicit register allocation**: Functions specify exact register needs
- **Reference counting**: Predictable memory usage
- **Actor model**: Natural parallelism and fault isolation
- **Prototype chains**: Flexible object model without classes

## Advanced Features

### Hot Code Reloading
- Functions can be replaced at runtime
- Process state is preserved during updates
- Gradual rollouts with version management

### Debugging Support
- Source location mapping in bytecode
- Breakpoint instruction support
- Process introspection capabilities

### Memory Management
- Reference counting with Rc/RefCell
- Garbage collection for cycles
- Process-local heaps for isolation

### Security Features
- Bytecode verification on load
- Resource access controls
- Process sandboxing
- FFI permission system

## Integration with IonPack

### Compilation Pipeline
```
Source Code (.ion) -> Parser -> AST -> Compiler -> Bytecode (.ionc) -> IonPack (.ionpack)
```

### Runtime Loading
```
IonPack -> Manifest -> Class Loading -> Function Deserialization -> VM Execution
```

### Module System
- Classes are packaged as individual .ionc files
- Dependencies resolved via IonPack manifest
- Hot-swappable modules for development
- Version-aware dependency resolution

### FFI Integration
- Native libraries bundled in IonPack
- Platform-specific library selection
- Dynamic library loading at runtime
- Function binding via FFI registry

For more details on IonPack format and CLI execution, see [IONPACK.md](IONPACK.md).

## CLI Tools

### IonVM CLI (`ionvm`)
The main CLI for executing IonPack files:y
```bash
# Execute an IonPack
ionvm run myapp.ionpack [args...]

# Execute with debug output
ionvm run --debug myapp.ionpack [args...]
ionvm run -d myapp.ionpack [args...]

# Show package information
ionvm info myapp.ionpack

# Show help
ionvm help
```

**Debug Output:**
The `--debug` flag enables detailed VM execution tracing:
- Process spawning and scheduling information
- Message sending and receiving operations
- Instruction execution flow
- Process state changes and mailbox operations
- Scheduler passes and reduction counting

```bash
# Example debug output
ionvm run --debug actors.ionpack
# [VM DEBUG] Spawning process 1 with function: "main"
# [VM DEBUG] Process 1 added to run queue. Total processes: 1, Queue length: 1
# [VM DEBUG] Scheduler pass 1. Run queue: [1]
# [VM DEBUG] Executing process 1
# [VM DEBUG] SPAWN: Function "worker" with 1 args
# [VM DEBUG] Spawning process 2 with function: "worker"
# ...
```

### Disassembler CLI (`iondis`)
Disassemble bytecode to human-readable text format:
```bash
# Disassemble a specific class from an IonPack
iondis myapp.ionpack ClassName

# Disassemble a raw .ionc bytecode file
iondis myclass.ionc

# Show help
iondis help
```

**Example Disassembly Output:**
```assembly
.bytecode
; Function: main (arity: 0, extra_regs: 1)
   0: LOAD_CONST r0, {name: "Alice", age: 30}
   1: RETURN r0
.end
```

### Complete Program: Fibonacci
```assembly
.bytecode
; Simplified fibonacci (arity: 1, extra_regs: 2)
   0: LOAD_CONST r1, 2.0        ; Multiplier
   1: MUL r2, r0, r1           ; r2 = input * 2
   2: RETURN r2                 ; Return result
.end
```

### Actor Communication
```assembly
.bytecode
; Worker process (arity: 0, extra_regs: 3)
   0: RECEIVE r0                ; Wait for work
   1: LOAD_CONST r1, 10.0      ; Process the work
   2: ADD r2, r0, r1           ; Add 10 to input
   3: RETURN r2                ; Return result
.end

; Main process (arity: 0, extra_regs: 5)
   0: LOAD_CONST r0, function:worker
   1: LOAD_CONST r1, '__vm:self'  ; Get self reference for worker
   2: SPAWN r2, r0, [r1]       ; Create worker with self ref
   3: LOAD_CONST r3, 5.0       ; Data to send
   4: SEND r2, r3              ; Send work
   5: RECEIVE r4               ; Get result
   6: RETURN r4                ; Return 15.0
```

**Note:** The `__vm:self` special value provides the current process reference, enabling proper bidirectional communication between actors.

### Pattern Matching
```assembly
.bytecode
; Pattern matching example (arity: 0, extra_regs: 2)
   0: LOAD_CONST r0, (ok, 42)  ; Result tuple
   1: MATCH r0, [
        ((error, _), +4),       ; Error handling
        ((ok, _), +1),          ; Success path
      ]
   2: LOAD_CONST r1, :error    ; Default
   3: RETURN r1
   4: LOAD_CONST r1, :success  ; Success case
   5: RETURN r1
   6: LOAD_CONST r1, :unknown  ; Error case
   7: RETURN r1
.end
```

## Debug and Development Features

### VM Debug Output
IonVM provides comprehensive debugging capabilities through the `debug` flag:

```rust
// Enable debug in code
let mut vm = IonVM::with_debug();
// or
vm.set_debug(true);
```

```bash
# Enable debug via CLI
ionvm run --debug myapp.ionpack
test_actors --debug
```

**Debug Information Includes:**
- **Process Lifecycle**: Spawning, termination, state changes
- **Message Passing**: Send/receive operations, mailbox sizes
- **Scheduling**: Process queue management, reduction counting
- **Instruction Execution**: Step-by-step bytecode execution
- **Special Values**: Resolution of `__vm:` prefixed values

### Development Workflow
1. **Write and compile** your IonVM program to bytecode
2. **Package** into IonPack format with dependencies
3. **Test** with debug output to verify actor communication
4. **Disassemble** bytecode for inspection and optimization
5. **Deploy** production builds without debug overhead

### Testing Actor Models
The VM includes comprehensive actor model testing:

```bash
# Test cross-process communication
cargo run --bin test_actors -- --debug

# Test with sample actor programs
ionvm run --debug unified-actors.ionpack
ionvm run --debug actors.ionpack
```

### Common Debug Patterns
Monitor process communication:
```
[VM DEBUG] SEND: Sending to process 2
[VM DEBUG] SEND: Message added to process 2 mailbox (size: 1)
[VM DEBUG] RECEIVE: Process 2 trying to receive into r0
[VM DEBUG] RECEIVE: Got message: Number(21.0)
```

Track process lifecycle:
```
[VM DEBUG] Spawning process 2 with function: "worker"
[VM DEBUG] Process 2 added to run queue
[VM DEBUG] Process 2 result: Continue
```

This bytecode format provides a solid foundation for a prototype-based, actor-model virtual machine with modern language features and comprehensive debugging support.
