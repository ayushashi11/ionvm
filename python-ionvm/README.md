# IonVM Python Library

Python library for creating IonVM bytecode and IonPack files.

## Features

- Generate IonVM binary bytecode
- Create IonPack archives (.ionpack files)  
- Python-friendly API for building IonVM programs
- Support for all IonVM instructions and value types
- Manifest generation and packaging

## Installation

```bash
pip install ionvm
```

## Quick Start

```python
from ionvm import Function, Instruction, Value, IonPackBuilder

# Create a simple function
function = Function(
    name="main",
    arity=0,
    extra_regs=1,
    instructions=[
        Instruction.load_const(0, Value.number(42)),
        Instruction.return_reg(0)
    ]
)

# Create an IonPack
builder = IonPackBuilder("hello-world", "1.0.0")
builder.main_class("Main")
builder.entry_point("main")
builder.add_class("Main", function)

with open("hello.ionpack", "wb") as f:
    builder.build(f)
```

## API Reference

### Instructions

All IonVM instructions are supported:

```python
# Arithmetic
Instruction.add(dst, a, b)
Instruction.sub(dst, a, b)
Instruction.mul(dst, a, b)
Instruction.div(dst, a, b)

# Memory
Instruction.load_const(reg, value)
Instruction.move(dst, src)

# Control flow
Instruction.jump(offset)
Instruction.jump_if_true(cond, offset)
Instruction.jump_if_false(cond, offset)
Instruction.return_reg(reg)

# Object operations
Instruction.get_prop(dst, obj, key)
Instruction.set_prop(obj, key, value)

# Function calls
Instruction.call(dst, func, args)

# Process operations
Instruction.spawn(dst, func, args)
Instruction.send(process, message)
Instruction.receive(dst)
```

### Values

All IonVM value types are supported:

```python
# Primitives
Value.number(42.0)
Value.boolean(True)
Value.atom("hello")
Value.unit()
Value.undefined()

# Complex types
Value.array([Value.number(1), Value.number(2)])
Value.object({"name": Value.atom("John"), "age": Value.number(30)})
Value.function_ref("my_function")
```

### Building IonPacks

```python
builder = IonPackBuilder("my-app", "1.0.0")
builder.description("My IonVM application")
builder.author("Developer")
builder.main_class("Main")
builder.entry_point("main")

# Add functions
builder.add_class("Main", main_function)
builder.add_class("Utils", utils_function)

# Add resources
builder.add_source("main.ion", source_code)
builder.add_resource("config.json", config_data)

# Build the package
with open("my-app.ionpack", "wb") as f:
    builder.build(f)
```
