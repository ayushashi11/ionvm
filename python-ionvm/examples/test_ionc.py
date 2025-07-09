#!/usr/bin/env python3
"""
Generate a standalone .ionc file for testing bytecode format compatibility.
"""
import sys
import os

# Add the library to the Python path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from ionvm import Function, Instruction, Value, BytecodeWriter

def main():
    # Create the same function as the Rust create_ionc.rs
    function = Function(
        name="test_function",
        arity=1,  # Takes one argument
        extra_regs=2,  # Need r1, r2 for calculation
        instructions=[
            Instruction.load_const(1, Value.number(10.0)),
            Instruction.add(2, 0, 1),  # r2 = arg + 10
            Instruction.return_reg(2)
        ]
    )
    
    # Create standalone .ionc file
    with open("test_python.ionc", "wb") as f:
        writer = BytecodeWriter(f)
        writer.write_function(function)
    
    print("Created test_python.ionc")
    print("You can test it with: cargo run --bin iondis test_python.ionc")

if __name__ == "__main__":
    main()
