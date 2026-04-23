# IonVM Python Library (AI-Optimized)

> **LLM Usage:** Guidance for bytecode generation and packaging. Use this when writing compilers or deployment tools.

## 🚀 Key Patterns

### 1. Define Functions
Functions require explicit register management (`arity` + `extra_regs`).
```python
from ionvm import Function, Instruction, Value
func = Function(
    name="my_func",
    arity=1, # r0
    extra_regs=1, # r1
    instructions=[
        Instruction.load_const(1, Value.number(10)),
        Instruction.add(1, 0, 1),
        Instruction.return_reg(1)
    ]
)
```

### 2. Build IonPack
```python
from ionvm import IonPackBuilder
builder = IonPackBuilder("app", "1.0.0")
builder.main_class("Main")
builder.add_class("Main", func)
builder.build(open("app.ionpack", "wb"))
```

## 🛠️ Components
- `ionvm.instruction`: Type-safe wrappers for all 30+ VM opcodes.
- `ionvm.value`: Factory methods for VM data types.
- `ionvm.ionpack`: High-level API for the `.ionpack` (ZIP) format.
- `ionvm.control_flow`: (Optional) Helpers for building if/while logic in bytecode.
