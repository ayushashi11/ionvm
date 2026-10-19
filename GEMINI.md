# IonVM AI Context & Foundation

> **AI Mandate:** This document provides foundational context for AI agents working on the IonVM project. All changes must respect the actor-model isolation and preemptive scheduling constraints.

## Project Essence
IonVM is a research virtual machine written in Rust, designed for **actor-model concurrency**. It features a preemptive scheduler, prototype-based objects, and a custom bytecode format.

## Strategic Code Map
- **Scheduler & Core VM:** `vmm/src/vm/` (The heartbeat of the system)
- **Instruction Set:** `vmm/src/instruction.rs` (The "API" of the VM)
- **Data Model:** `vmm/src/value/` (How everything is stored)
- **FFI Layer:** `vm-ffi/` (Integration with native code/Python)
- **Frontend/Compiler:** `mesh/` (The high-level language research area)
- **Python Bindings:** `python-ionvm/` (Tooling and bytecode generation)

## Critical Constraints for AI
1. **Reduction Budget:** Every instruction costs a "reduction". Default timeslice is 3 instructions. Do not bypass the budget check in `vmm/src/vm/scheduler.rs`.
2. **Reference Counting:** The VM uses `Rc<RefCell<...>>` for shared state. Be careful with cycles.
3. **Bytecode Versioning:** Changes to the instruction set require updates in serialization (`vmm/src/bytecode_binary.rs`) and the Python library (`python-ionvm/`).
4. **Actor Isolation:** Processes must not share memory directly; all communication is via `SEND`/`RECEIVE` (mailboxes).

## Common Tasks for AI
- **Adding an Instruction:**
  - Update `Instruction` enum in `vmm/src/instruction.rs`.
  - Implement logic in `vmm/src/vm/ops/`.
  - Update serializer/deserializer in `vmm/src/bytecode_binary.rs`.
  - Update Python side in `python-ionvm/ionvm/`.
- **Debugging:**
  - Use `ionvm --debug` to see instruction-level traces.
  - Check `vmm/src/integration_tests.rs` for regression testing.
