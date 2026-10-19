# IonVM Architecture & Execution Pipeline

> **Architecture Overview:** High-performance, concurrent actor virtual machine featuring a dual-tier execution engine (tail-call direct dispatch bytecode interpreter + LLVM JIT compiler with hardware SIMD vectorization).

---

## 1. Concurrency & Scheduling Architecture

### 1.1 Multi-Threaded Work-Stealing Scheduler
The IonVM runtime is designed around a multi-worker concurrent actor model:
- **Scheduler Core (`vmm/src/vm/mod.rs`):**
  - Manages a pool of background worker threads (`worker_loop`).
  - Processes are queued in a shared, thread-safe run queue (`Arc<Mutex<VecDeque<usize>>>`).
  - When runnable, processes are dequeued by available worker threads and executed for a designated reduction slice.
- **Process Isolation (`vmm/src/vm/process.rs`):**
  - Each `Process` owns:
    - `pid`: Unique numeric process identifier.
    - `mailbox`: Thread-safe FIFO message queue (`Arc<RwLock<VecDeque<Value>>>`).
    - `frame_stack`: Call frames (`Frame`) tracking `function_index`, `pc`, and local registers.
    - `status`: Lifecycle state (`Runnable`, `WaitingForMessage`, `Suspended`, `Exited`).
  - Inter-process communication is non-blocking via `Opcode::Send` and suspended-until-arrival via `Opcode::Receive`.

---

## 2. Dual-Tier Execution Engine

```
                          ┌─────────────────────────┐
                          │   Bytecode Binary File  │
                          └────────────┬────────────┘
                                       │
                                       ▼
                          ┌─────────────────────────┐
                          │   Bytecode Deserializer │
                          │ (vmm/src/bytecode/serde)│
                          └────────────┬────────────┘
                                       │
                                       ▼
                          ┌─────────────────────────┐
                          │     Function Bytecode   │
                          └────────────┬────────────┘
                                       │
                         [optimizer::optimize_function]
                                       │
             ┌─────────────────────────┴─────────────────────────┐
             │                                                   │
             ▼                                                   ▼
┌───────────────────────────┐                       ┌───────────────────────────┐
│ Tier 1: Bytecode Engine   │                       │ Tier 2: LLVM JIT Engine   │
│ - DISPATCH_TABLE (110 ops)│                       │ - Inkwell LLVM Compiler   │
│ - Tail-Call Direct Dispatch│                      │ - Loop & SLP Vectorization│
│ - Zero Stack Overhead     │                       │ - Native x86_64 / AVX-512 │
└───────────────────────────┘                       └───────────────────────────┘
```

### 2.1 Tier 1: Direct Tail-Call Dispatch Interpreter (`vmm/src/vm/executor.rs`)
- **Dispatch Table:** An array of 110 function pointers (`DISPATCH_TABLE: [HandlerFn; 110]`), indexed directly by opcode integer values.
- **Tail-Call Execution:** Every opcode handler finishes by invoking `become fetch_and_dispatch(ctx)`. On modern LLVM / Rust releases, tail calls are compiled into direct jumps (`jmp`), eliminating C-stack call overhead and avoiding call stack overflows.

### 2.2 Tier 2: LLVM JIT Compiler (`vmm/src/jit/llvm_translator.rs`)
- **Native JIT ABI:**
  ```rust
  pub type NativeJitFn = unsafe extern "C" fn(
      registers: *mut Value,
      args: *const Value,
      out_res: *mut Value
  );
  ```
- **Optimization Passes:**
  Employs LLVM `OptimizationLevel::Aggressive` (`-O3`) combined with:
  - Loop vectorization (`pass_opts.set_loop_vectorization(true)`)
  - SLP vectorization (`pass_opts.set_loop_slp_vectorization(true)`)
  - Loop unrolling and interleaving
- **Selective Argument Pointer Retrieval:**
  The JIT entry block statically inspects the function instructions. It only queries raw data pointers (`vm_get_intarray_data_ptr` or `vm_get_floatarray_data_ptr`) for arguments that are genuinely accessed by typed array opcodes. Scalar functions emit **zero** array data calls, preventing spurious function invocations and null pointer hazards.
- **Direct Pointer Loads for Vectorization:**
  With dedicated array opcodes (`IArrayGet`, `FArrayGet`), LLVM translates array loads into direct pointer GEPs + loads without branching or fallback wrappers. This gives LLVM clean loop access patterns, enabling automatic SIMD vectorization.
- **Type-Preserving Return Handling:**
  `Opcode::Ret` dynamically inspects the register's discriminant tag (`tag 0 = Int`, `tag 1 = Float`, `tag 2 = Bool`, `tag 6 = Nil`) and copies both tag and 64-bit payload directly into the caller's `out_res` buffer, allowing math stubs to return arbitrary numeric values or sentinel codes (`-1`).

### 2.3 Decoupled Mathematical Stub Pattern (Actor-JIT Bridge)
Worker functions often coordinate actor concurrency (`Send`, `Receive`, `Spawn`) alongside heavy numerical computation. Because actor primitives involve VM runtime side-effects, the function as a whole is ineligible for LLVM JIT compilation.

The runtime solves this via the **Math Stub Pattern**:
1. **Mathematical Stub Extraction:** The inner search loop is extracted into a pure mathematical stub function (`find_next_stub(cur, end, cache) -> i64`). It contains zero actor primitives and operates exclusively via typed arithmetic, fused branches, and `IArrayGet`.
2. **LLVM JIT Optimization:** The stub is compiled directly by LLVM with `-O3`, vectorizing the loop and executing with direct pointer dereferencing on contiguous arrays.
3. **Transparent Rewrite (`CallJit`):** During initialization, the VM detects calls from the actor driver to the JIT-compiled stub and rewrites `Opcode::Call` to `Opcode::CallJit`.
4. **Execution Performance:** Over 99.99% of compute cycles run in native machine code, with the VM driver intervening only when matches arrive to stream them across actor mailboxes.

---

## 3. Data Representation & Memory Layout

### 3.1 Value System (`vmm/src/lang/types.rs`)
`Value` is declared as `#[repr(C, u8)]` for high-performance C-interoperability and predictable memory offsets:
- `Byte 0`: `u8` discriminant tag (`0 = Int`, `1 = Float`, `2 = Bool`, `3 = String`, `4 = Object`, `5 = Function`, `6 = Nil`, `7 = Array`, `8 = IntArray`, `9 = FloatArray`).
- `Bytes 8..15`: 64-bit payload (direct integer `i64`, direct float `f64`, or 64-bit pointer to heap container).

### 3.2 Unboxed Primitive Arrays
To eliminate pointer chasing and dynamic typing overhead:
- `Value::IntArray(Arc<RwLock<Vec<i64>>>)`: Direct contiguous vector of 64-bit signed integers.
- `Value::FloatArray(Arc<RwLock<Vec<f64>>>)`: Direct contiguous vector of 64-bit floating-point numbers.
- `Value::Array(Arc<RwLock<Vec<Value>>>)`: General heterogeneous dynamic array.

---

## 4. Optimization Pipeline (`vmm/src/vm/optimizer.rs`)

The optimization pass runs on functions before execution:
1. **Type & Register Flow Inference:** Tracks known types (`Int`, `Float`, `IntArray`, `FloatArray`) through registers without making unfounded assumptions about unknown function arguments.
2. **Opcode Specialization:**
   - Rewrites polymorphic `GetIndex` to `IArrayGet` when operating on `IntArray`.
   - Rewrites polymorphic `GetIndex` to `FArrayGet` when operating on `FloatArray`.
   - Rewrites polymorphic arithmetic (`Add`, `Sub`, `Mul`, `Div`, `Rem`) to fast specialized integer (`IAddRRR`, `ISubRRR`) or float (`FAddRRR`) opcodes.
   - Fuses comparisons and jumps into single instructions (`IGtJmpRR`, `IGtJmpRC`, `IEqJmpRC`, etc.).
3. **Loop Vectorization & Unrolling:**
   - Analyzes loop headers and latches (`transform_loop_to_simd`).
   - Generates 4-lane vector unrolling with stride updates and cleanup epilogue loops.
   - Visible directly in `iondis <file.bin>` under `Optimized Bytecode (In-VM Loop Vectorization: 4-Lane SIMD Unrolled)`.
4. **Hardware SIMD Kernel Integration:**
   - Detects CPU capabilities via `is_x86_feature_detected!("avx512f")` and executes vectorized batch computation routines (`simd_munchausen_range`).
   - High-throughput string SIMD via AVX-512BW (64 bytes/cycle) and AVX2 (32 bytes/cycle) for character transformations (`simd_str_to_upper`, `simd_str_to_lower`), fast byte counting (`simd_str_count_byte`), vector equality (`simd_str_eq`), and substring probe searches (`simd_str_find`).
   - Achieves up to 4.44 GB/s throughput in character counting (13.5x faster than Node.js, 6.6x faster than Java OpenJDK 25) and 14.5 GB/s in string equality comparisons.

---

## 5. Source Tree Layout

- `vmm/src/bytecode/opcode.rs`: Full `Opcode` enumeration (110 opcodes) with string conversions.
- `vmm/src/bytecode/instruction.rs`: `Instruction`, `OperandType`, and `Bytecode` structures.
- `vmm/src/bytecode/serde.rs`: Binary serialization and deserialization codec.
- `vmm/src/bytecode/disasm.rs`: Bytecode disassembler with LLVM IR inspector.
- `vmm/src/lang/types.rs`: Core data types (`Value`, `Object`, `Function`, `Property`).
- `vmm/src/vm/executor.rs`: Interpreter execution context, dispatch table, and opcode handlers.
- `vmm/src/vm/optimizer.rs`: Bytecode optimization, specialization, and loop vectorizer.
- `vmm/src/vm/simd.rs`: Hardware SIMD and AVX-512 accelerated numerical kernels.
- `vmm/src/jit/llvm_translator.rs`: LLVM IR code generator and JIT compilation engine.
- `vmm/src/jit/translator.rs`: JIT eligibility verifier and compilation interface.
- `vm-ffi/`: Foreign Function Interface and modular standard library bridge.
- `docs/SAFETY.md`: Complete audit of all `unsafe` blocks, invariants, failure modes, and mitigations.
