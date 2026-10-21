# IonVM API Reference (AI-Optimized)

> **LLM Usage:** Precise signatures, types, and standard library module references for VM extension and tooling.

---

## 🦀 Rust Core (`vmm`)

### `IonVM`
- `new(bytecode: Bytecode) -> Result<Self, String>`: Initializes runtime with decoded bytecode program.
- `spawn_process(func_idx: usize, args: Vec<Value>) -> usize`: Returns PID of newly scheduled process.
- `run() -> Result<(), String>`: Executes the multi-threaded work-stealing scheduler until all processes terminate or suspend.

### `Value` (`vmm::lang::types::Value`)
- `Int(i64)`: 64-bit unboxed signed integer.
- `Float(f64)`: 64-bit unboxed IEEE 754 floating-point number.
- `Bool(bool)`: Boolean literal (`true` / `false`).
- `String(String)`: UTF-8 encoded heap string.
- `Nil`: Unit / null value.
- `Array(Arc<RwLock<Vec<Value>>>)`: Dynamic heterogenous array.
- `IntArray(Arc<RwLock<Vec<i64>>>)`: Specialized unboxed 64-bit integer array.
- `FloatArray(Arc<RwLock<Vec<f64>>>)`: Specialized unboxed 64-bit float array.
- `Object(Arc<RwLock<Object>>)`: Prototype-backed property hashmap with fast indexed slots.
- `Function(Arc<RwLock<Function>>)`: First-class bytecode or native function.

### Bytecode Instructions
- `Instruction::new(opcode: Opcode, operands: Vec<OperandType>)`
- `Instruction::format_value(val: OperandType, spec: OperandType, dst: OperandType)`: Formats value using standard specifier (`.2f`, `x`, `b`, `o`, `05d`, `>10`, `,`, `_`, `.1%`) into destination string register.
- `Instruction::build_string(operands: Vec<OperandType>)`: Single-pass concatenation of $N$ string parts into destination register (supports `RegisterList` or multiple operands).
- `OperandType`: `Register(usize)`, `Constant(Value)`, `Argument(usize)`, `Offset(i64)`, `Function(String)`, `SelfProc`, `This`, `RegisterList(Vec<usize>)`.

---

## 🌉 FFI Bridge & Standard Library (`vm-ffi`)

### `RawFfiFunction` Trait (C++ Binary Compatibility)
The base trait for all foreign functions, designed to have an exact 9-slot vtable matching C++ abstract base classes (`drop`, `size`, `align`, `call`, `name`, `arity`, `return_type`, `is_variadic`, `description`):
```rust
pub trait RawFfiFunction: Send + Sync {
    fn call(&self, ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult;
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    fn return_type(&self) -> FfiType { FfiType::Any }
    fn is_variadic(&self) -> bool { false }
    fn description(&self) -> Option<&str> { None }
}
```

### `FfiFunction` Trait (Generator & Coroutine Inheritance)
Directly inherits from `RawFfiFunction` using Rust trait inheritance (`pub trait FfiFunction: RawFfiFunction`), adding cooperative coroutine generator resumption without altering the low-level C++ vtable of `RawFfiFunction`:
```rust
pub trait FfiFunction: RawFfiFunction {
    /// Resume execution of a suspended generator function with new input
    fn resume(&self, ctx: &mut dyn FfiContext, input: FfiValue) -> FfiResult {
        let _ = (ctx, input);
        Err(FfiError::RuntimeError(format!(
            "Function '{}' is not a resumable generator",
            self.name()
        )))
    }

    /// Check if this function is a resumable generator / coroutine
    fn is_resumable(&self) -> bool { false }
}
```

### `FfiReturn` & `FfiResult`
FFI calls return an `FfiResult` (`Result<FfiReturn, FfiError>`) supporting cooperative VM interactions:
```rust
pub enum FfiReturn {
    /// Direct return value passed back to VM destination register
    Value(FfiValue),
    /// Request VM to call another function/callback and yield its result
    Callback { target: String, args: Vec<FfiValue> },
    /// Cooperative poll: call target until truthy/finished or timeout
    WaitAndCheck { target: String, args: Vec<FfiValue>, timeout_ms: Option<u64> },
}

pub type FfiResult = Result<FfiReturn, FfiError>;
```
- **Automatic Conversions:** Any `FfiValue` automatically converts into `FfiReturn::Value(val)` via `From<FfiValue>` and `.into()`.
- **Constructors:** `FfiReturn::value(val)`, `FfiReturn::callback(target, args)`, `FfiReturn::wait_and_check(target, args, timeout_ms)`.

### `FfiCoroutineFunction<F>` (Rust Generator Coroutines)
Wraps native Rust `#[coroutine]` closures into full `FfiFunction` implementations without requiring OS fibers or assembly:
```rust
use vm_ffi::{FfiCoroutineFunction, FfiValue, FfiReturn};

let gen = FfiCoroutineFunction::new(
    "range_gen",
    0,
    |mut input: FfiValue| #[coroutine] static move || {
        let mut i = 0;
        while i < 3 {
            input = yield Ok(FfiValue::Integer(i).into());
            i += 1;
        }
        Ok(FfiValue::String("done".into()).into())
    }
);
```

### `FfiValue`
```rust
pub enum FfiValue {
    Integer(i64),
    Float(f64),
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
    Native(Rc<dyn std::any::Any>),
}
```

### Standard Library Modules

The standard library is organized into modular subsystems and automatically registered in `FfiRegistry::with_stdlib()`. Functions can be invoked by both exact PascalCase and snake_case aliases:

#### 1. Math Module (`math`)
- `Sqrt(Number) -> Number` (`sqrt`)
- `Abs(Number) -> Number` (`abs`)
- `Sin(Number) -> Number` (`sin`)
- `Cos(Number) -> Number` (`cos`)
- `Floor(Number) -> Number` (`floor`)
- `Ceil(Number) -> Number` (`ceil`)
- `Round(Number) -> Number` (`round`)
- `Max(Number, Number) -> Number` (`max`)
- `Min(Number, Number) -> Number` (`min`)
- `Pow(base: Number, exp: Number) -> Number` (`pow`)
- `Log(Number) -> Number` (`log`)

#### Native SIMD String Operations (Bytecode Opcodes)
String manipulation is performed directly by the VM using hardware-accelerated AVX2 / AVX-512 vectorization rather than FFI:
- `SUpperRR Reg(src), Reg(dst)`: Vectorized ASCII uppercase (in-place zero allocation if `src == dst`).
- `SLowerRR Reg(src), Reg(dst)`: Vectorized ASCII lowercase (in-place zero allocation if `src == dst`).
- `SCountByteRRR Reg(str), Reg(byte), Reg(dst)`: AVX-512BW / AVX2 masked byte frequency counter.
- `SEqRRR Reg(str1), Reg(str2), Reg(dst)`: Vector equality comparison with early mismatch exit.
- `SFindRRR Reg(haystack), Reg(needle), Reg(dst)`: Dual-byte vectorized probe substring finder.
- `FormatValue Reg(val), Reg(spec), Reg(dst)`: High-performance string formatter using standard format specifiers: `[[fill]align][sign][#][0][width][,|_][.prec][type]`. Types include: `c` (integer unicode character conversion, e.g. 65 -> 'A'), `d` (decimal), `x`/`X` (hex), `b` (binary), `o` (octal), `f` (float fixed), `e`/`E` (scientific), `%` (percentage).
- `BuildString Pieces..., Reg(dst)`: Concatenates arbitrary operands (string constants, registers, register lists) into a single string.

#### 2. I/O Module (`io`)
- `Print(...Any) -> Unit` (`print`)
- `PrintLn(...Any) -> Unit` (`println`)
- `PrintF(format: String, ...Any) -> Unit` (`printf`)
- `Debug(Any) -> Unit` (`debug`)
- `Eprint(Any) -> Unit` (`eprint`)

#### 3. Terminal Graphics & Input Module (`chafa`)
- `ChafaRender(rgb_data: Array|String, width: Number, height: Number, term_cols: Number, term_rows: Number) -> String` (`chafa_render`): Converts RGB byte buffer to terminal ANSI escape strings using system chafa or truecolor half-blocks.
- `ChafaPrint(rgb_data: Array|String, width: Number, height: Number, term_cols: Number, term_rows: Number) -> Unit` (`chafa_print`): Directly displays RGB buffer on terminal using Kitty graphics protocol, Sixel, iTerm2, or truecolor.
- `DisplayImage(rgb_data: Array|String, width: Number, height: Number, term_cols: Number, term_rows: Number) -> Unit` (`display_image`): Alias for `ChafaPrint`.
- `ReadInput([prompt: String]) -> String` (`read_input`): Reads a full line of text from standard input with optional prompt.
- `ReadKey([prompt: String]) -> String` (`read_key`): Reads a single raw keypress without requiring Enter.

#### 4. Time Module (`time`)
- `GetTime() -> Number` (`get_time`): Unix epoch timestamp in fractional seconds.

#### 5. Operating System Module (`os`)
- `GetEnv(String) -> String | Undefined` (`get_env`)
- `SetEnv(key: String, value: String) -> Unit` (`set_env`)
- `OsName() -> String` (`os_name`): Host OS ("linux", "windows", "macos").
- `OsArch() -> String` (`os_arch`): CPU architecture ("x86_64", "aarch64").
- `GetPid() -> Number` (`get_pid`): Current OS process identifier.

#### 6. File System Module (`fs`)
- `ReadFile(path: String) -> String` (`read_file`)
- `WriteFile(path: String, content: String) -> Number` (`write_file`)
- `AppendFile(path: String, content: String) -> Number` (`append_file`)
- `FileExists(path: String) -> Boolean` (`file_exists`)
- `RemoveFile(path: String) -> Boolean` (`remove_file`)

#### 7. Hashing Module (`hash`)
- `Fnv1a64(String) -> Number` (`fnv1a64`): 64-bit FNV-1a non-cryptographic hash.
- `Crc32(String) -> Number` (`crc32`): IEEE 802.3 32-bit checksum.

---

## 🔌 C-ABI Foreign Plugin Interface (`include/ion_c_abi.h`)

IonVM natively loads external shared objects (`.so`, `.dylib`, `.dll`) and in-memory ELF objects written in C, C++, Fortran, or Pascal without wrappers.

### Plugin Header & Magic
Plugins must export the magic identifier:
```c
#include "ion_c_abi.h"

uint32_t IONVM_PLUGIN_MAGIC = 0x494F4E56;
```

### Tagged Value (`IonCValue`)
A 16-byte C-compatible tagged union:
```c
typedef struct IonCValue {
    int32_t tag;         /* ION_VAL_NIL, INT, FLOAT, BOOL, STRING, POINTER */
    int32_t _reserved;   /* 8-byte alignment padding */
    union {
        int64_t int_val;
        double float_val;
        int32_t bool_val;
        const char* string_val;
        void* ptr_val;
    } data;
} IonCValue;
```

### Native Function Signature
```c
typedef int32_t (*IonCFunction)(const IonCValue* args, size_t argc, IonCValue* out);
```

### Plugin Initialization Hook
```c
int32_t ionvm_plugin_init(void* reg_ctx, IonCRegisterCallback reg_cb) {
    reg_cb(reg_ctx, "my_mod.compute", my_func, 2, ION_TYPE_INT);
    return 0;
}
```

### Cooperative C-to-VM Fiber Yield
C functions can yield execution back to the VM to invoke callbacks via `ion_c_yield`:
```c
IonCValue cb_args[1] = { { .tag = ION_VAL_FLOAT, .data.float_val = 144.0 } };
IonCValue cb_res;
ion_c_yield("sqrt", cb_args, 1, &cb_res);
```
Under the hood, IonVM manages a dedicated fiber execution stack for the C function, swapping CPU registers (`%rbx`, `%rsp`, `%rbp`, `%r12-%r15`) and returning to user code once the VM callback finishes.

