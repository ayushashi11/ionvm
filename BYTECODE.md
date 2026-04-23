# IonVM Bytecode Specification (AI-Optimized)

> **LLM Usage:** This file defines the Ground Truth for the VM's execution logic. Use this for code generation or validation of compiler output.

## 🏗️ Architecture
- **Type:** Register-based.
- **Register File:** `r0...rN`. Allocated per-frame (`arity + extra_regs`).
- **Data Types:** Number (f64), Boolean, Atom, Unit, Undefined, Array, Object, Tuple, Function, Closure, Process.

## 📜 Instruction Set (ISA)

### Memory & Registry
- `LOAD_CONST dst, value`: Loads `value` into `dst`. Special atoms: `__vm:self`, `__vm:pid`.
- `MOVE dst, src`: `registers[dst] = registers[src]`.

### Arithmetic (dst, a, b)
- `ADD`, `SUB`, `MUL`, `DIV`: Floating point math. `ADD` on strings = concat. `MUL` on strings = repeat.

### Comparison & Logic
- `EQUAL`, `NOT_EQUAL`, `LESS_THAN`, `LESS_EQUAL`, `GREATER_THAN`, `GREATER_EQUAL`.
- `AND`, `OR` (truthy evaluation), `NOT`.

### Control Flow
- `JUMP offset`: Relative signed offset to IP.
- `JUMP_IF_TRUE/FALSE cond, offset`: Conditional branch.
- `CALL dst, func, [args...]`: Calls `func` (bytecode/FFI). Stores result in `dst`.
- `RETURN reg`: Returns value in `reg` to caller.

### Actor Model
- `SPAWN dst, func, [args...]`: Creates new process. Returns PID to `dst`.
- `SEND proc, msg`: Enqueues `msg` in `proc`'s mailbox.
- `RECEIVE dst`: Blocks until mailbox is non-empty. Pops to `dst`.
- `RECEIVE_WITH_TIMEOUT dst, timeout, res`: Wait `timeout` ms. `res` gets success bool.
- `YIELD`: Cooperative preemption point.

### Objects & Arrays
- `OBJECT_INIT dst, {key: val...}`: Creates object.
- `ARRAY_INIT dst, [regs...]`: Creates array from registers.
- `GET_PROP dst, obj, key`: `dst = obj[key]` (traverses prototypes).
- `SET_PROP obj, key, val`: `obj[key] = val`.

### Pattern Matching
- `MATCH src, [(pattern, offset)...]`: Jump to first matching pattern offset.

## 🔢 Binary Encoding (v1)
- **Magic:** `IONBC\x01\x00\x00`
- **Instruction Format:** `[u8: opcode] [u32: args...]` (Variable length based on opcode).
- **Value Serialization:** See `vmm/src/bytecode_binary.rs`.

## 📈 Reduction Costs
- **Light (1):** Arithmetic, Jumps, Move, Load.
- **Medium (2):** Call, Receive, Link.
- **Heavy (4):** Spawn, Send.
