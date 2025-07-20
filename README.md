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
test_actors --debug
# IonVM: Actor-Model Virtual Machine and Toolchain

IonVM is a research virtual machine and toolchain for actor-model concurrency, message-passing, and prototype-based objects. It is designed for experimentation with process scheduling, message delivery, and dynamic language features. The project includes a bytecode VM, a packaging format (IonPack), a Python FFI, and a suite of actor-oriented examples and tests.

## Features

- **Actor Model:** Lightweight processes, message-passing, mailbox per process, blocking and timeout receives, process linking.
- **Preemptive Scheduler:** Configurable timeslice (default: 3 instructions), round-robin scheduling, fair interleaving of processes.
- **Prototype-Based Objects:** Dynamic objects, property access, prototype chains, magic methods.
- **Bytecode Instruction Set:** Arithmetic, logic, control flow, function calls, process management, message passing, pattern matching.
- **IonPack Format:** Portable package format for bytecode, source, and metadata.
- **FFI:** Python bindings for embedding and extension.
- **Comprehensive Tests:** Unit and integration tests for VM, actors, control flow, and message delivery semantics.

## Project Structure

- `vmm/` — Main Rust VM and IonPack toolchain
  - `src/vm.rs` — Core VM, scheduler, process model, instruction execution
  - `src/value.rs` — Value types, objects, processes, function/closure model
  - `src/ionpack.rs` — IonPack packaging and loading
  - `src/bin/create_sample.rs` — IonPack generator for all demo scenarios
  - `src/bin/ionvm.rs` — CLI runner for IonPack files
  - `src/bin/iondis.rs` — Disassembler for IonPack bytecode
  - `src/vm_timeout_tests.rs` — Receive-with-timeout and process wakeup tests
- `python-ionvm/` — Python FFI bindings and examples
- `examples/` — Example IonPack and Python actor programs
- `target/` — Build artifacts

## Key Concepts

### Processes and Scheduling
- Each process has its own stack, registers, mailbox, and status.
- The scheduler is preemptive: after a configurable number of instructions (default: 3), the current process is preempted and the next runnable process is scheduled.
- Blocking instructions (e.g., `RECEIVE`, `RECEIVE_WITH_TIMEOUT`) suspend the process until a message arrives or a timeout expires.
- Message delivery via `SEND` wakes up waiting processes and ensures they are scheduled.

### Instruction Set (Selected)
- `SPAWN(dst, func, args)` — Spawn a new process running `func` with `args`.
- `SEND(proc, msg)` — Send a message to a process's mailbox.
- `RECEIVE(dst)` — Receive a message (blocks if mailbox is empty).
- `RECEIVE_WITH_TIMEOUT(dst, timeout_reg, result_reg)` — Receive with timeout; sets result to `true` if message received, `false` if timeout.
- `CALL(dst, func, args)` — Call a function or FFI.
- `RETURN(reg)` — Return from function.
- `GETPROP/SETPROP` — Object property access.
- `MATCH` — Pattern matching.
- `YIELD` — Voluntary process yield.

### IonPack Format
- Bundles bytecode, source, and metadata for portable deployment.
- Supports multi-function classes and cross-process actor patterns.

### Python FFI
- Exposes IonVM as a Python module for embedding and extension.
- Example: run IonPack files, spawn actors, send/receive messages from Python.

## Example Workflows

### Simple Actor
```ion
function main() {
    const worker = spawn("Worker", [self]);
    send(worker, 42);
    const result = receive();
    return result;
}

function worker(coordinator) {
    const msg = receive();
    send(coordinator, msg * 2);
    return msg * 2;
}
```

### Preemptive Scheduling
- With timeslice=3, processes interleave every 3 instructions, ensuring fairness and responsiveness.
- Example: a long-running process cannot starve others; all actors make progress.

### Receive with Timeout
- `RECEIVE_WITH_TIMEOUT` allows a process to wait for a message with a timeout, resuming with a default value if no message arrives.

## Building and Running

### Build the VM and Tools
```sh
cargo build --release
```

### Generate Example IonPacks
```sh
cargo run --bin create_sample
```

### Run an IonPack
```sh
cargo run --bin ionvm run actors.ionpack
```

### Disassemble an IonPack
```sh
cargo run --bin iondis actors.ionpack Main
```

## Testing

Run all tests (including actor, control flow, and timeout tests):
```sh
cargo test
```

## Extending and Research Directions
- **Configurable Timeslice:** Change `IonVM.timeslice` for different scheduling behaviors.
- **Process Priorities:** Future work: add priority-based scheduling.
- **Advanced Pattern Matching:** Extend `MATCH` for richer actor protocols.
- **FFI Extensions:** Add more host language bindings.

## License
MIT or Apache 2.0 (choose your preferred license).

## Authors
IonVM Developer and contributors.
</edits>
