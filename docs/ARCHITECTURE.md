# IonVM Architecture Map (AI-Optimized)

> **LLM Usage:** Mental model for IonVM. Use this to understand component interactions and lifecycle management.

## 🕒 Scheduler (Preemptive)
- **Mechanism:** Round-robin with reduction counting.
- **Cycle:** `run()` -> `pop_front(run_queue)` -> `execute_process_slice()` -> `handle_execution_result()`.
- **Preemption:** Occurs when `proc.budget` hits 0. Budget is reset to `IonVM.reduction_budget` each turn.
- **Blocking:** Instructions like `RECEIVE` return `ExecutionResult::Blocked`, moving the PID from `run_queue` to a waiting state.

## 🧵 Process Model
- **Isolation:** Each `Process` has its own `mailbox`, `frames` (stack), and `environments` (closures).
- **Communication:** Pure asynchronous message passing.
- **Status:** `Runnable`, `WaitingForMessage`, `Suspended`, `Exited`.
- **Entry Point:** Defined by a `Function` object (bytecode or FFI).

## 💎 Value & Object System
- **Prototype Chain:** `Object` has an optional `prototype: Option<Rc<RefCell<Object>>>`.
- **Property Access:**
  - `GetProp`: Recursive lookup up the prototype chain.
  - `SetProp`: Always sets on the local object (shadowing).
- **Memory:** Shared ownership via `Rc<RefCell<T>>`. Be cautious of reference cycles in object graphs.

## 🌉 FFI & Integration
- **Registry:** `vm-ffi/src/lib.rs` (`FfiRegistry`) stores name-to-function mappings.
- **Bridge:** `vm-ffi/src/bridge.rs` converts between `Value` (VM) and `FfiValue` (Native).
- **Call Flow:** `Instruction::Call` -> Check if function is FFI -> `vm.ffi_registry.call()`.

## 📂 Key Source Files for Agents
- `vmm/src/vm/scheduler.rs`: Scheduler loop and unblocking logic.
- `vmm/src/vm/ops/`: Implementation of every bytecode instruction.
- `vmm/src/value/object.rs`: Object property and prototype logic.
- `vmm/src/bytecode_binary.rs`: The binary codec for bytecode.
