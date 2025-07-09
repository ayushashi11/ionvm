#!/usr/bin/env python3
"""
Simple test to verify our new comparison and logical instructions work properly.
"""

import sys
import os
sys.path.append(os.path.join(os.path.dirname(__file__), 'python-ionvm'))

from ionvm import IonPackBuilder, Function, Instruction, Value


def create_simple_test():
    """Create a simple test without control flow to verify the basic functionality."""
    builder = IonPackBuilder("simple-test", "1.0.0")
    builder.description("Simple test of comparison instructions")
    builder.entry_point("main")
    builder.main_class("Main")
    
    # Test function that does basic comparisons without complex control flow
    test_instructions = [
        # Load test values
        Instruction.load_const(0, Value.number(25.0)),   # r0 = 25
        Instruction.load_const(1, Value.number(10.0)),   # r1 = 10
        Instruction.load_const(2, Value.number(100.0)),  # r2 = 100
        
        # Test: 25 < 10 (should be false)
        Instruction.less_than(3, 0, 1),  # r3 = false
        
        # Test: 25 < 100 (should be true) 
        Instruction.less_than(4, 0, 2),  # r4 = true
        
        # Test: logical AND (false && true = false)
        Instruction.logical_and(5, 3, 4),  # r5 = false
        
        # Test: logical OR (false || true = true)
        Instruction.logical_or(6, 3, 4),   # r6 = true
        
        # Return the OR result (should be true)
        Instruction.return_reg(6)
    ]
    
    test_func = Function("simple_test", 0, 7, test_instructions)
    builder.add_class("TestModule", test_func)
    
    # Main function
    main_instructions = [
        Instruction.load_const(0, Value.function_ref("TestModule:simple_test")),
        Instruction.call(1, 0, []),
        Instruction.return_reg(1)
    ]
    
    main_func = Function("main", 0, 2, main_instructions)
    builder.add_class("Main", main_func)
    
    # Create the IonPack
    import io
    stream = io.BytesIO()
    builder.build(stream)
    ionpack = stream.getvalue()
    
    # Save to file
    with open('simple_test.ionpack', 'wb') as f:
        f.write(ionpack)
    
    print("Created simple_test.ionpack successfully!")
    print("Expected result: true (Boolean)")
    print("Logic: (25 < 10) || (25 < 100) = false || true = true")


if __name__ == "__main__":
    create_simple_test()
