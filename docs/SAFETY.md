# Safety and `unsafe` Code Audit in IonVM

IonVM is designed with a high-performance architecture combining:
1. **Actor concurrency** with isolated register and frame stacks.
2. **SIMD vectorization** using AVX2 and AVX-512 hardware intrinsics.
3. **LLVM Just-In-Time (JIT) compilation** directly emitting native x86-64 machine code.
4. **POSIX network primitives** for direct OS socket communication.

To achieve maximum throughput and sub-millisecond execution times, IonVM uses Rust's `unsafe` keyword in carefully bounded locations. This document provides a complete inventory of every `unsafe` block, their safety invariants, preconditions, potential failure modes, and architectural mitigations.

---

## 1. Safety Inventory Summary

| Subsystem / File | Purpose | Primary Invariants | Failure Risk Level |
|---|---|---|---|
| `vmm/src/vm/simd.rs` | AVX2 / AVX-512 vector arithmetic, string kernels, and Munchausen search | Pointer alignment/bounds, UTF-8 byte invariance | Medium |
| `vmm/src/vm/executor.rs` | Zero-copy JIT argument marshaling and C-ABI function dispatch | Frame stack lifetime, uninitialized buffer safety | High |
| `vmm/src/jit/llvm_translator.rs` | LLVM GEP calculations, native JIT execution, and C-ABI helper callbacks | Memory layout alignment, lock guard lifetime | High |
| `vmm/src/builtin/net.rs` | Direct libc POSIX socket calls (`socket`, `bind`, `read`, `write`, `close`) | OS file descriptor lifecycle | Low–Medium |
| `vmm/src/builtin/native/loader.rs` | Dynamic plugin loading via `libloading` and anonymous in-memory `memfd_create` | Library lifecycle, symbol signature validation | High |
| `vmm/src/builtin/native/fiber.rs` | Raw x86_64 assembly stack-switching (`switch_context`) for C-ABI coroutines | Callee-saved register restore, 16-byte stack alignment | High |
| `vmm/src/builtin/native/c_abi.rs` | Tagged union marshaling (`IonCValue`) and raw C-string pointer conversions | NUL-terminated strings, union tag validation | Medium |
| `vm-ffi/` | External function interface, Rust coroutine generators, stdlib | Safe state machines (`#[coroutine]`), argument checking | Safe (0 unsafe in core) |

---

## 2. SIMD Vector Engine (`vmm/src/vm/simd.rs`)

### 2.1 Hardware Intrinsics & Unaligned Loads
- **Code Locations**: `simd_vadd_f`, `simd_vsub_f`, `simd_vmul_f`, `simd_vdiv_f`, `simd_vdot_f`, `simd_vsum_f`, `simd_str_count_byte`, `simd_str_to_upper_inplace`, `simd_str_to_lower_inplace`, `simd_str_eq`, `simd_str_find`.
- **Intrinsics Used**: `_mm256_loadu_pd`, `_mm256_storeu_pd`, `_mm512_loadu_si512`, `_mm512_storeu_si512`, `_mm256_loadu_si256`, `_mm256_storeu_si256`.
- **Safety Invariant**:
  - Unaligned vector loads (`loadu`) and stores (`storeu`) are used exclusively. Therefore, pointers do not require 32-byte or 64-byte alignment.
  - The loop conditions guarantee that reading `i + 4` elements (`f64`), `i + 32` bytes, or `i + 64` bytes remains strictly `<= len`.
  - Slice pointers (`a.as_ptr().add(i)`) remain valid and within slice allocations for the lifetime of the vector operation.
- **Potential Failure Modes**:
  - **Out-of-bounds pointer read/write**: Prevented by the loop guard `while i + N <= len`. The remaining trailing elements `i < len` are handled safely in scalar mode.
  - **Hardware feature mismatch**: Prevented by runtime `is_x86_feature_detected!("avx2")` and `is_x86_feature_detected!("avx512bw")` CPUID checks prior to entering `unsafe` blocks.

### 2.2 UTF-8 String Transformations (`from_utf8_unchecked`)
- **Code Locations**: `simd_str_to_upper`, `simd_str_to_lower`, `simd_str_to_upper_mut`, `simd_str_to_lower_mut`.
- **Safety Proof**:
  - Multi-byte UTF-8 code points use leading bytes `0xC0..=0xFF` and continuation bytes `0x80..=0xBF`, where the highest bit (bit 7) is always set (`>= 128`).
  - ASCII uppercase (`0x41..=0x5A`) and lowercase (`0x61..=0x7A`) have bit 7 cleared (`< 128`).
  - Adding or subtracting 32 (`0x20`) only transforms bytes within `0x41..=0x5A` or `0x61..=0x7A`.
  - Continuation bytes and multi-byte headers are never modified. Thus, an input string that is valid UTF-8 remains strictly valid UTF-8 after ASCII in-place casing.
- **Potential Failure Modes**:
  - **Non-ASCII Unicode casing**: Casing that alters byte length (such as German `ß` $\to$ `SS`, or ligatures) or non-ASCII characters (e.g. `é`, `ü`) are not transformed. This is not a memory safety violation (does not cause Undefined Behavior), but is a semantic domain limitation.
  - **Arbitrary byte slices**: If `simd_str_to_upper_inplace` is ever passed a malformed byte buffer from external C FFI and subsequently converted via `from_utf8_unchecked`, Rust's `&str` UTF-8 invariant would be violated.
  - *Mitigation*: The function is internal to the VM, and only accepts valid Rust `&str` or `String` inputs.

### 2.3 SIMD Munchausen AVX-512 Loop
- **Code Location**: `check_simd_range_avx512`
- **Safety Invariant**:
  - Direct pointer access to `TABLE_DATA.as_ptr()` loads 16 pre-computed 32-bit powers.
  - Permutation index lookup `_mm512_permutexvar_epi32(rem, table)` relies on `rem` being within `0..=9`.
- **Potential Failure Modes**:
  - **Digit Limit Overflow**: The inner loop executes exactly 7 digit-extraction iterations (`for _ in 0..7`).
  - *Invariant Violation*: Any number $\ge 10,000,000$ (8 or more decimal digits) will have higher digits truncated in the AVX-512 register, causing missed matches.
  - *Mitigation*: The input range should be bounded to $10,000,000$, or the loop unroll depth increased dynamically based on `end`.

---

## 3. Bytecode Executor & JIT Bridge (`vmm/src/vm/executor.rs`)

### 3.1 Uninitialized Memory & Zero-Copy Stack Arguments
- **Code Locations**: `op_call` (`op_call_jit` path) and `run_compiled_function`.
- **Implementation**:
  ```rust
  let mut stack_args: [std::mem::MaybeUninit<Value>; 8] = unsafe {
      std::mem::MaybeUninit::uninit().assume_init()
  };
  for i in 0..arg_count {
      let val = match &inst.operands[2 + i] {
          OperandType::Register(r) => unsafe { std::ptr::read(reg_val as *const Value) },
          ...
      };
      stack_args[i].write(val);
  }
  // Native JIT execution...
  std::mem::forget(stack_args);
  ```
- **Safety Rationale**:
  - Cloning `Value` requires incrementing reference counts (`Rc<RefCell<...>>`) or copying heap buffers (`String`, `Vec<i64>`).
  - Bitwise copying via `std::ptr::read` avoids allocations when passing arguments into LLVM-generated C functions.
  - `std::mem::forget(stack_args)` ensures the destructors of the copied `Value`s are never invoked when `stack_args` falls out of scope, preventing double-free.
- **Critical Failure Modes**:
  1. **Stack Buffer Overflow**: `stack_args` is a fixed array of length 8. If bytecode passes `arg_count > 8`, `stack_args[i]` will cause a panic or out-of-bounds write.
     - *Mitigation*: Add an assertion or clamp: if `arg_count > 8`, fallback to heap-allocated `Vec<Value>` or return an `ExecutionResult::Error`.
  2. **Premature Register Invalidation**: If the native JIT function were to mutate `args` or invoke an operation that reallocates the frame's `registers` or `arguments` vector, the raw pointers passed in `stack_args` could become dangling.
     - *Mitigation*: LLVM translation emits read-only GEP loads for `args_param`; writing to `args` is forbidden in compiled IR.

### 3.2 JIT Function Execution Pointer
- **Code Location**: `native_fn(regs_ptr, args_ptr, out_res_ptr)`
- **Safety Invariant**:
  - `native_fn` is a pointer returned by LLVM ExecutionEngine (`ee.get_function::<NativeJitFn>(&fn_name)`).
  - The pointer conforms strictly to the C calling convention:
    `extern "C" fn(registers: *mut Value, args: *const Value, out_res: *mut Value)`.
- **Potential Failure Modes**:
  - **Memory Corruption / Segfault**: If LLVM JIT compilation generated invalid GEP offsets or bad register indexing (e.g. accessing register 35 when only 32 stack registers were allocated), the native CPU instructions will fault.
  - *Mitigation*: Stack registers in `executor.rs` are allocated with size 32: `let mut stack_regs = [const { Value::Nil }; 32];`. The LLVM translator must validate that all referenced registers satisfy `r < 32`.

---

## 4. LLVM Translator (`vmm/src/jit/llvm_translator.rs`)

### 4.1 C-ABI Native Helper Callbacks
- **Code Locations**: `vm_get_obj_prop_int`, `vm_set_obj_prop_int`, `vm_get_obj_prop_flt`, `vm_set_obj_prop_flt`, `vm_get_intarray_data_ptr`, `vm_get_floatarray_data_ptr`, `vm_simd_find_next`.
- **Signature**: `#[unsafe(no_mangle)] pub extern "C" fn(...)`
- **Safety Invariant**:
  - Pointers passed from JIT code must point to valid `Value` instances or be `NULL`.
  - Every helper begins with defensive null checks (`if val_ptr.is_null() { return 0; }`).
- **Critical Failure Mode: Dangling Data Pointers**:
  - In `vm_get_intarray_data_ptr`:
    ```rust
    if let Value::IntArray(arr) = &*val_ptr {
        if let Ok(borrowed) = arr.read() {
            return borrowed.as_ptr();
        }
    }
    ```
  - The read guard `borrowed` is dropped at the end of the `if let` block!
  - The returned raw pointer `borrowed.as_ptr()` points to the internal heap buffer of `Vec<i64>`.
  - If another actor thread writes to or reallocates the array while the JIT code executes GEP instructions on this pointer, a data race and use-after-free occurs.
  - *Mitigation*: This pointer is currently only used for immutable stub loops (such as `TABLE_DATA`). For general concurrent arrays, arrays must be pinned or synchronized during JIT execution.

### 4.2 Raw Struct Offsets & GEP Layout
- **Code Locations**: `build_gep` in `set_reg_int`, `get_reg_int`, `set_reg_float`, `get_reg_float`, `set_reg_bool`, and return handling.
- **Assumed Memory Layout**:
  - Tag offset: `0` (`i64` discriminant).
  - Payload offset: `8` (`i64` or `f64` value payload).
  - Total `Value` size: `std::mem::size_of::<Value>()` (typically 32 bytes on x86-64 due to enum payload alignment).
- **Potential Failure Modes**:
  - If Rust's internal enum representation or padding changes across compiler versions, hardcoded offsets (`int_offset = 8`) could write into invalid fields or corrupt pointers.
  - *Mitigation*: The translator dynamically computes `size_of_val` using `context.i64_type().const_int(std::mem::size_of::<Value>() as u64, false)`.

---

## 5. POSIX Network Builtins (`vmm/src/builtin/net.rs`)

### 5.1 Direct Libc Syscalls
- **Code Locations**: `tcp_listen`, `tcp_accept`, `tcp_recv`, `tcp_send`, `tcp_close`.
- **Syscalls**: `libc::socket`, `libc::bind`, `libc::listen`, `libc::accept`, `libc::read`, `libc::write`, `libc::close`, `libc::setsockopt`.
- **Safety Invariants**:
  - Sockets are created with `SOCK_CLOEXEC` to prevent file descriptor leakage across `fork`/`exec`.
  - Buffer bounds are explicitly passed to `libc::read` and `libc::write` matching allocated vector lengths.
- **Potential Failure Modes**:
  - **Arbitrary File Descriptor Access**: Bytecode passes raw integer values for file descriptors (`fd: i32`). If bytecode provides `fd = 0` (stdin) or `fd = 1` (stdout) or an arbitrary open file descriptor, `tcp_close` or `tcp_write` can close or corrupt unrelated system resources.
  - **Thread Blocking**: POSIX socket calls are blocking. If an actor calls `tcp_accept` or `tcp_recv` without incoming network traffic, the entire worker thread is blocked.
  - *Mitigation*: Introduce an opaque socket table (`SocketHandle(u32)`) mapped to internal descriptors rather than passing raw OS integer file descriptors to user bytecode.

---

## 6. Native Plugins & C-ABI Fibers (`vmm/src/builtin/native/`)

### 6.1 Assembly Fiber Context Switching (`fiber.rs`)
- **Code Location**: `vmm/src/builtin/native/fiber.rs` (`switch_context`, `fiber_entry_trampoline`).
- **Assembly Routine (`switch_context`)**:
  ```asm
  pushq %rbp; pushq %rbx; pushq %r12; pushq %r13; pushq %r14; pushq %r15
  movq %rsp, (%rdi)
  movq (%rsi), %rsp
  popq %r15; popq %r14; popq %r13; popq %r12; popq %rbx; popq %rbp
  ret
  ```
- **Safety Invariants**:
  - **Callee-Saved Registers**: The System V AMD64 ABI mandates that `%rbp`, `%rbx`, `%r12`, `%r13`, `%r14`, and `%r15` are callee-saved. All 6 registers are pushed onto the current stack before the stack pointer is updated, and popped in reverse order upon resumption.
  - **16-Byte Stack Alignment**: On x86_64, the stack must be 16-byte aligned before executing `call` instructions. The initial stack pointer setup in `Fiber::new` aligns the allocated buffer to a 16-byte boundary and adjusts for the fake return address and pushed register frames.
  - **Direction Flag**: The direction flag (`DF`) is cleared (`cld`) prior to any C function execution to prevent string instruction corruption (`rep stosb` / `rep movsb`).
- **Potential Failure Modes**:
  - **Stack Overflow**: The native C function executes on an allocated heap stack (default 512 KB to 2 MB). Unbounded recursion in foreign C code could overflow the fiber stack and corrupt heap memory.
  - *Mitigation*: A memory guard page can be mapped with `mprotect(PROT_NONE)` at the base of the allocated stack to trigger a deterministic `SIGSEGV` instead of silent heap corruption.
  - **Thread Migration**: Fibers hold CPU register state tied to the calling thread. A fiber must never be resumed on a different OS thread than the one on which it suspended without synchronizing TLS and thread-local state.

### 6.2 Dynamic Shared Library Loading (`loader.rs`)
- **Code Location**: `vmm/src/builtin/native/loader.rs` (`load_shared_library`, `load_in_memory_library`).
- **Safety Invariants**:
  - **Magic Verification**: Before inspecting plugin symbols, the loader verifies that `IONVM_PLUGIN_MAGIC == 0x494F4E56`. This prevents executing arbitrary shared libraries that do not conform to IonVM's plugin specification.
  - **Handle Lifetime Pinning**: Loaded `libloading::Library` handles are permanently held in a global `Mutex<Vec<Library>>` (`REGISTERED_HANDLES`). Handles are never dropped while the VM process is alive, preventing use-after-free or invalid instruction execution from unmapped `.text` pages.
  - **Anonymous In-Memory Loading**: When loading raw binary ELF shared objects, `memfd_create` is used with `fcntl(F_ADD_SEALS, F_SEAL_WRITE | F_SEAL_SHRINK | F_SEAL_GROW)` to prevent concurrent modification of the executable image in memory.

### 6.3 Contrast: Safe Rust Coroutines (`vm-ffi`)
Unlike foreign C functions that require manual stack manipulation and assembly context switching, native Rust generator functions implemented via `vm_ffi::FfiCoroutineFunction` use Rust's native `#[coroutine]` language feature:
- Coroutine closures are transformed by the Rust compiler into 100% safe, type-checked enum state machines.
- All local variables across `yield` points are stored within the compiler-generated generator struct.
- There is zero assembly, zero stack pointer swapping, and zero possibility of stack corruption or register leakage.

---

## 7. Actionable Safety Recommendations

1. **Arg-Count Guard in Executor**:
   Add an explicit check in `op_call_jit` and `run_compiled_function`:
   ```rust
   if arg_count > 8 {
       return ExecutionResult::Error("JIT call supports at most 8 arguments".to_string());
   }
   ```
2. **Fiber Stack Guard Pages**:
   Allocate fiber stacks via `mmap` with a 4KB `PROT_NONE` guard page at the stack bottom to trap stack overflows deterministically.
3. **Safe Array Memory Pinning**:
   Ensure `vm_get_intarray_data_ptr` either retains the read lock across the JIT invocation or only operates on immutable / frozen array constants.
4. **Socket Descriptor Table**:
   Wrap raw network sockets in a managed `Process`-owned handle table to prevent arbitrary descriptor closure.

