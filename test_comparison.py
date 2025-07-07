#!/usr/bin/env python3
"""
Test script to generate IonPack that uses comparison and logical operations.
"""

import sys
import os
sys.path.append(os.path.join(os.path.dirname(__file__), 'python-ionvm'))

from ionvm import IonPackBuilder, Function, Instruction, Value

def create_comparison_test():
    """Create a simple test that uses comparison and logical operations."""
    builder = IonPackBuilder("comparison-test", "1.0.0")
    builder.description("Test comparison and logical operations")
    builder.entry_point("main")
    builder.main_class("Main")
    
    # Test function that compares two numbers and returns a boolean
    test_instructions = [
        # Load two numbers to compare
        Instruction.load_const(0, Value.number(10.0)),
        Instruction.load_const(1, Value.number(5.0)),
        
        # Test: 10 > 5 (should be true)
        Instruction.greater_than(2, 0, 1),
        
        # Test: 5 < 10 (should be true) 
        Instruction.less_than(3, 1, 0),
        
        # Test: logical AND of the two results (true && true = true)
        Instruction.logical_and(4, 2, 3),
        
        # Load false for testing OR
        Instruction.load_const(5, Value.boolean(False)),
        
        # Test: true || false = true
        Instruction.logical_or(6, 4, 5),
        
        # Test: NOT false = true
        Instruction.logical_not(7, 5),
        
        # Test equality: check if 10 == 10
        Instruction.load_const(8, Value.number(10.0)),
        Instruction.equal(9, 0, 8),
        
        # Return the final equality result
        Instruction.return_reg(9)
    ]
    
    test_func = Function("test_comparisons", 0, 10, test_instructions)
    builder.add_class("TestModule", test_func)
    
    # Main function that just calls the test
    main_instructions = [
        Instruction.load_const(0, Value.function_ref("TestModule:test_comparisons")),
        Instruction.call(1, 0, []),  # Call with no arguments
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
    with open('comparison_test.ionpack', 'wb') as f:
        f.write(ionpack)
    
    print("Created comparison_test.ionpack successfully!")
    print("Expected result: true (Boolean)")

if __name__ == "__main__":
    create_comparison_test()
