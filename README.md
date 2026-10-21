# IonVM: AI-Optimized Project Entry Point

> **LLM Context:** IonVM is a research virtual machine (Rust) for actor-model concurrency. It implements preemptive scheduling (reduction counting), prototype-based objects, and a custom register bytecode.

## 🗺️ System Map
- **`vmm/` (Core VM):**
  - `src/vm/`: Preemptive scheduler, tail-call direct dispatch interpreter, and bytecode optimizer.
  - `src/jit/`: LLVM-based JIT compiler with loop and SLP SIMD vectorization.
  - `src/lang/`: High-performance object model, unboxed arrays (`IntArray`, `FloatArray`), pattern matching engine (`matcher.rs`), and string formatting (`format.rs`).
  - `src/builtin/native/`: Polyglot C-ABI plugin dynamic loader (`loader.rs`), assembly fibers (`fiber.rs`), and C interop (`c_abi.rs`).
- **`vm-ffi/` (Native Bridge & Stdlib):**
  - Two-tier trait system: `RawFfiFunction` (C++ vtable compatibility) and `FfiFunction` (generator coroutine resumption).
  - Native Rust coroutine generators (`FfiCoroutineFunction`) using `#![feature(coroutines)]`.
  - Modular standard library (`math`, `io`, `time`, `os`, `fs`, `hash`, `chafa` terminal graphics).
  - Cooperative FFI returns (`FfiReturn`: `Value`, `Callback`, `WaitAndCheck`).
- **`include/` (Native C Headers):**
  - `ion_c_abi.h`: Standard C header for foreign plugins (C, C++, Fortran, Pascal).
- **`python-ionvm/` (Tooling):** Python library for generating IonPack files and bytecode.
- **`mesh/` (Compiler):** (Research) Python-based compiler for the `.ion` language.

## ⚙️ Core Technical Mandates
1. **Preemptive Concurrency:** Processes are scheduled via round-robin. Every instruction has a `reduction_cost`. The budget per turn is defined in `IonVM::reduction_budget`.
2. **Actor Isolation:** No shared memory between processes. All communication must use `SEND`/`RECEIVE` via mailboxes (`VecDeque<Value>`).
3. **Prototype Inheritance:** Objects use a prototype chain. `GetProp` traverses the chain; `SetProp` is local.
4. **Register Architecture:** VM functions use a fixed number of registers (`arity` + `extra_regs`). Register 0 to `arity-1` are arguments.
5. **Polyglot & Generator FFI:** Native C-ABI plugins with fiber yields and native Rust coroutines without assembly fibers.

## 🛠️ Operational Commands
- **Build All:** `cargo build`
- **Run Sample:** `cargo run --bin ionvm run actors.ionpack`
- **Test Core VM:** `cargo test -p vmm`
- **Test FFI & Stdlib:** `cargo test -p vm-ffi`
- **Test Python:** `cd python-ionvm && pytest`
- **Debug Trace:** `ionvm --debug run <file>.ionpack`

## 📚 Key Specifications
- [BYTECODE.md](BYTECODE.md) - Register-based ISA, opcodes, and binary format.
- [IONPACK.md](IONPACK.md) - ZIP-based packaging and manifest spec.
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) - Scheduler, JIT pipeline, SIMD kernels, and FFI architecture.
- [API_REFERENCE.md](docs/API_REFERENCE.md) - Complete API reference for VM, FFI traits, standard library, and C plugins.
- [SAFETY.md](docs/SAFETY.md) - Complete audit of `unsafe` blocks, assembly fibers, memory layout, and invariants.
