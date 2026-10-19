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
- `OperandType`: `Register(usize)`, `Constant(Value)`, `Argument(usize)`, `Offset(i64)`, `Function(String)`, `SelfProc`, `This`.

---

## 🌉 FFI Bridge & Standard Library (`vm-ffi`)

### `FfiFunction` Trait
```rust
pub trait FfiFunction {
    fn call(&self, ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult;
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    fn is_variadic(&self) -> bool { false }
    fn description(&self) -> Option<&str> { None }
}
```

### `FfiValue`
```rust
pub enum FfiValue {
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

#### 2. I/O Module (`io`)
- `Print(...Any) -> Unit` (`print`)
- `PrintLn(...Any) -> Unit` (`println`)
- `PrintF(format: String, ...Any) -> Unit` (`printf`)
- `Debug(Any) -> Unit` (`debug`)
- `Eprint(Any) -> Unit` (`eprint`)

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
