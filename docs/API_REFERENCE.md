# IonVM API Reference (AI-Optimized)

> **LLM Usage:** Precise signatures and types for VM extension and tooling.

## 🦀 Rust Core (`vmm`)

### `IonVM`
- `new()`: Returns default VM.
- `spawn_process(func: Rc<RefCell<Function>>, args: Vec<Value>) -> usize`: Returns PID.
- `run()`: Executes the scheduler until idle.
- `spawn_main_process(func: Function) -> Result<Value, String>`: Synchronous execution to completion.

### `Value` & `Primitive`
- `Value`: `Primitive(P)`, `Tuple(Rc<Vec>)`, `Array(Rc<RefCell<Vec>>)`, `Object(Rc<RefCell>)`, `Function(Rc<RefCell>)`, `Closure(Rc)`, `Process(Rc<RefCell>)`.
- `Primitive`: `Number(f64)`, `Boolean(bool)`, `String(String)`, `Atom(String)`, `Complex(Complex64)`, `Unit`, `Undefined`.

### `Function`
- `new_bytecode(name, arity, extra_regs, bytecode: Vec<Instruction>)`
- `new_ffi(name, arity, function_name)`

## 🐍 Python SDK (`python-ionvm`)

### `ionvm.Instruction`
- Static methods mapping 1:1 to Rust `Instruction` enum:
  - `load_const(dst, val)`, `add(dst, a, b)`, `spawn(dst, func, args)`, `send(proc, msg)`, etc.

### `ionvm.IonPackBuilder`
- `add_class(name, func)`: Adds compiled function to package.
- `main_class(name)`: Sets entry point class.
- `build(output_stream)`: Serializes to ZIP format.

## 🌉 FFI Bridge (`vm-ffi`)

### `FfiFunction` Trait
- `call(args: Vec<FfiValue>) -> FfiResult`
- `name()`, `arity()`

### `FfiValue`
- VM `Value` equivalent for native boundary. Mapping logic in `vm-ffi/src/bridge.rs`.
