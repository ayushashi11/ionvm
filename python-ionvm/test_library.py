#!/usr/bin/env python3
"""
Test script for the IonVM Python library.
"""
import sys
import os

# Add the library to the Python path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from ionvm import Function, Instruction, Value, IonPackBuilder

def test_basic_function():
    """Test creating a basic function."""
    print("Testing basic function creation...")
    
    function = Function(
        name="test",
        arity=0,
        extra_regs=1,
        instructions=[
            Instruction.load_const(0, Value.number(42)),
            Instruction.return_reg(0)
        ]
    )
    
    print(f"Created function: {function}")
    assert function.name == "test"
    assert function.arity == 0
    assert function.extra_regs == 1
    assert len(function.instructions) == 2
    print("✓ Basic function test passed")

def test_value_types():
    """Test creating different value types."""
    print("\nTesting value types...")
    
    values = [
        Value.number(42.5),
        Value.boolean(True),
        Value.atom("hello"),
        Value.unit(),
        Value.undefined(),
        Value.array([Value.number(1), Value.number(2)]),
        Value.object({"name": Value.atom("test")}),
        Value.function_ref("my_func")
    ]
    
    for i, val in enumerate(values):
        print(f"  Value {i}: {val}")
    
    print("✓ Value types test passed")

def test_instructions():
    """Test creating different instruction types."""
    print("\nTesting instruction types...")
    
    instructions = [
        Instruction.load_const(0, Value.number(42)),
        Instruction.move(1, 0),
        Instruction.add(2, 0, 1),
        Instruction.sub(3, 2, 1),
        Instruction.mul(4, 2, 3),
        Instruction.div(5, 4, 2),
        Instruction.get_prop(6, 0, 1),
        Instruction.set_prop(0, 1, 2),
        Instruction.call(7, 0, [1, 2]),
        Instruction.return_reg(7),
        Instruction.jump(5),
        Instruction.jump_if_true(0, 3),
        Instruction.jump_if_false(0, -2),
        Instruction.spawn(8, 0, [1]),
        Instruction.send(8, 2),
        Instruction.receive(9),
        Instruction.nop()
    ]
    
    for i, instr in enumerate(instructions):
        print(f"  Instruction {i}: {instr}")
    
    print("✓ Instruction types test passed")

def test_ionpack_creation():
    """Test creating an IonPack."""
    print("\nTesting IonPack creation...")
    
    # Create a simple function
    function = Function(
        name="main",
        arity=0,
        extra_regs=1,
        instructions=[
            Instruction.load_const(0, Value.number(123)),
            Instruction.return_reg(0)
        ]
    )
    
    # Create builder
    builder = IonPackBuilder("test-package", "1.0.0")
    builder.main_class("Main")
    builder.entry_point("main")
    builder.description("Test package")
    builder.author("Test Author")
    
    builder.add_class("Main", function)
    builder.add_source("main.ion", "function main() { return 123; }")
    
    # Build to a file
    with open("test_python.ionpack", "wb") as f:
        builder.build(f)
    
    print("✓ IonPack creation test passed")
    print("  Created test_python.ionpack")

def main():
    """Run all tests."""
    print("Running IonVM Python library tests...\n")
    
    test_basic_function()
    test_value_types()
    test_instructions()
    test_ionpack_creation()
    
    print("\n🎉 All tests passed!")
    print("\nYou can test the generated IonPack with:")
    print("  cargo run --bin ionvm run test_python.ionpack")

if __name__ == "__main__":
    main()
