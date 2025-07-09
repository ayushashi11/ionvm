#!/usr/bin/env python3
"""
Simplified demo focusing on if-else with lazy evaluation.
"""

import sys
import os

# Add the python-ionvm directory to the path
sys.path.append(os.path.join(os.path.dirname(__file__), 'python-ionvm'))

from ionvm import (
    IonPackBuilder, 
    Function, 
    Instruction, 
    Value,
    IfElseBuilder
)


def create_simple_demo():
    """
    Create a simple function demonstrating if-else with lazy evaluation.
    """
    builder = IfElseBuilder()
    
    # First condition: check if input (r0) equals 1
    builder.if_condition_lazy([
        Instruction.load_const(1, Value.number(1)),
        Instruction.equal(2, 0, 1)
    ], 2)
    
    # First block
    builder.add_instruction(Instruction.load_const(3, Value.atom("__stdlib:debug")))
    builder.add_instruction(Instruction.load_const(4, Value.string("Matched 1")))
    builder.add_instruction(Instruction.call(5, 3, [4]))
    builder.add_instruction(Instruction.load_const(6, Value.string("first")))
    builder.add_instruction(Instruction.return_reg(6))
    
    # Second condition: check if input equals 42 (lazy evaluation)
    builder.elif_condition_lazy([
        Instruction.load_const(7, Value.number(42)),
        Instruction.equal(8, 0, 7)
    ], 8)
    
    # Second block  
    builder.add_instruction(Instruction.load_const(9, Value.atom("__stdlib:debug")))
    builder.add_instruction(Instruction.load_const(10, Value.string("Matched 42")))
    builder.add_instruction(Instruction.call(11, 9, [10]))
    builder.add_instruction(Instruction.load_const(12, Value.string("second")))
    builder.add_instruction(Instruction.return_reg(12))
    
    # Else block
    builder.else_block()
    builder.add_instruction(Instruction.load_const(13, Value.atom("__stdlib:debug")))
    builder.add_instruction(Instruction.load_const(14, Value.string("No match")))
    builder.add_instruction(Instruction.call(15, 13, [14]))
    builder.add_instruction(Instruction.load_const(16, Value.string("default")))
    builder.add_instruction(Instruction.return_reg(16))
    
    instructions = builder.build()
    return Function(
        name="simple_demo",
        arity=1,
        extra_regs=17,
        instructions=instructions
    )


def create_main_function():
    """
    Create the main function that tests different scenarios.
    """
    instructions = [
        # Test 1: input = 1
        Instruction.load_const(0, Value.atom("__stdlib:debug")),
        Instruction.load_const(1, Value.string("=== Test 1: Input = 1 ===")),
        Instruction.call(2, 0, [1]),
        
        Instruction.load_const(3, Value.number(1)),
        Instruction.load_const(4, Value.atom("__function_ref:Demo:simple_demo")),
        Instruction.call(5, 4, [3]),
        
        # Test 2: input = 42
        Instruction.load_const(6, Value.atom("__stdlib:debug")),
        Instruction.load_const(7, Value.string("=== Test 2: Input = 42 ===")),
        Instruction.call(8, 6, [7]),
        
        Instruction.load_const(9, Value.number(42)),
        Instruction.load_const(10, Value.atom("__function_ref:Demo:simple_demo")),
        Instruction.call(11, 10, [9]),
        
        # Test 3: input = 99
        Instruction.load_const(12, Value.atom("__stdlib:debug")),
        Instruction.load_const(13, Value.string("=== Test 3: Input = 99 ===")),
        Instruction.call(14, 12, [13]),
        
        Instruction.load_const(15, Value.number(99)),
        Instruction.load_const(16, Value.atom("__function_ref:Demo:simple_demo")),
        Instruction.call(17, 16, [15]),
        
        # Final message
        Instruction.load_const(18, Value.atom("__stdlib:debug")),
        Instruction.load_const(19, Value.string("All tests completed!")),
        Instruction.call(20, 18, [19]),
        Instruction.return_reg(20)
    ]
    
    return Function(
        name="main",
        arity=0,
        extra_regs=21,
        instructions=instructions
    )


def main():
    """
    Main function to create and run the demo.
    """
    print("Simple IonVM Control Flow Demo")
    print("Creating IonPack with lazy evaluation...")
    
    # Create the IonPack
    builder = IonPackBuilder("simple-control-flow", "1.0.0")
    builder.main_class("Main")
    builder.entry_point("main")
    builder.description("Simple control flow demo with lazy evaluation")
    builder.author("IonVM Demo")
    
    # Add functions
    builder.add_class("Demo", create_simple_demo())
    builder.add_class("Main", create_main_function())
    
    # Build and save
    output_file = "simple_control_flow_demo.ionpack"
    with open(output_file, "wb") as f:
        builder.build(f)
    
    print(f"IonPack created: {output_file}")
    print("You can run it with: cargo run --bin ionvm run simple_control_flow_demo.ionpack")


if __name__ == "__main__":
    main()
