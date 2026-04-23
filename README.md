# IonVM: AI-Optimized Project Entry Point

> **LLM Context:** IonVM is a research virtual machine (Rust) for actor-model concurrency. It implements preemptive scheduling (reduction counting), prototype-based objects, and a custom register bytecode.

## 🗺️ System Map
- **`vmm/` (Core VM):**
  - `src/vm/`: Scheduler and execution loop.
  - `src/value/`: Prototype object model and value types.
  - `src/instruction.rs`: Instruction set definition.
  - `src/bytecode_binary.rs`: Serialization logic.
- **`vm-ffi/` (Native Bridge):** Bridge between Rust/VM and external functions.
- **`python-ionvm/` (Tooling):** Python library for generating IonPack files and bytecode.
- **`mesh/` (Compiler):** (Research) Python-based compiler for the `.ion` language.

## ⚙️ Core Technical Mandates
1. **Preemptive Concurrency:** Processes are scheduled via round-robin. Every instruction has a `reduction_cost`. The budget per turn is defined in `IonVM::reduction_budget`.
2. **Actor Isolation:** No shared memory between processes. All communication must use `SEND`/`RECEIVE` via mailboxes (`VecDeque<Value>`).
3. **Prototype Inheritance:** Objects use a prototype chain. `GetProp` traverses the chain; `SetProp` is local.
4. **Register Architecture:** VM functions use a fixed number of registers (`arity` + `extra_regs`). Register 0 to `arity-1` are arguments.

## 🛠️ Operational Commands
- **Build All:** `cargo build`
- **Run Sample:** `cargo run --bin ionvm run actors.ionpack`
- **Test Core:** `cargo test -p vmm`
- **Test Python:** `cd python-ionvm && pytest`
- **Debug Trace:** `ionvm --debug run <file>.ionpack`

## 📚 Key Specifications
- [BYTECODE.md](BYTECODE.md) - Register-based ISA and binary format.
- [IONPACK.md](IONPACK.md) - ZIP-based packaging and manifest spec.
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) - Deep dive into scheduler and memory.
