#!/usr/bin/env python3
"""
Simple hello world example using the IonVM Python library.
"""
import sys
import os

# Add the library to the Python path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from ionvm import Function, Instruction, Value, IonPackBuilder

def main():
    # Create a simple function that returns 42
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
    builder.description("Simple hello world example")
    builder.author("Python IonVM Library")
    builder.add_class("Main", function)
    
    # Add source code for reference
    builder.add_source("main.ion", """
function main() {
    return 42;
}
""")
    
    # Build the package
    with open("hello_python.ionpack", "wb") as f:
        builder.build(f)
    
    print("Created hello_python.ionpack successfully!")
    print("You can run it with: cargo run --bin ionvm run hello_python.ionpack")

if __name__ == "__main__":
    main()
