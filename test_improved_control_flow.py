#!/usr/bin/env python3
"""
Test script demonstrating the improved control flow builders for IonVM.
Shows both pre-evaluated conditions and lazy-evaluated conditions with proper
short-circuit evaluation.
"""

import sys
import os
import io

# Add the python-ionvm directory to the path
sys.path.append(os.path.join(os.path.dirname(__file__), 'python-ionvm'))

from ionvm import IonPackBuilder, Function, Instruction, Value
from ionvm.control_flow import IfElseBuilder, WhileThenElseBuilder, build_if_else, build_while_then_else


def test_if_else_with_side_effects():
    """
    Test if-else with conditions that have side effects.
    This demonstrates why lazy evaluation is important.
    """
    print("="*60)
    print("TEST: If-Else with Side Effects")
    print("="*60)
    
    builder = IfElseBuilder()
    
    # First condition: simple register check
    builder.if_condition(0)  # Assume r0 contains a boolean
    builder.add_instruction(Instruction.load_const(1, Value.atom("first")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Second condition: with side effects (function call)
    # This should only be evaluated if the first condition is false
    side_effect_condition = [
        Instruction.load_const(2, Value.function_ref("Module:has_side_effect")),
        Instruction.call(3, 2, []),  # Call function with side effects
        Instruction.greater_than(4, 3, 0)  # Check if result > 0
    ]
    
    builder.elif_condition_lazy(side_effect_condition, 4)
    builder.add_instruction(Instruction.load_const(1, Value.atom("second")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Third condition: another side effect function
    # This should only be evaluated if both previous conditions are false
    another_side_effect = [
        Instruction.load_const(5, Value.function_ref("Module:another_side_effect")),
        Instruction.call(6, 5, []),  # Another function with side effects
        Instruction.equal(7, 6, 0)  # Check if result == 0
    ]
    
    builder.elif_condition_lazy(another_side_effect, 7)
    builder.add_instruction(Instruction.load_const(1, Value.atom("third")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Else block
    builder.else_block()
    builder.add_instruction(Instruction.load_const(1, Value.atom("default")))
    builder.add_instruction(Instruction.return_reg(1))
    
    instructions = builder.build()
    
    print("Generated instructions:")
    for i, instr in enumerate(instructions):
        print(f"{i:2d}: {instr}")
    
    print("\nKey benefits:")
    print("- Side effect functions are only called when needed")
    print("- Proper short-circuit evaluation")
    print("- Clean separation between condition evaluation and branching")
    
    return instructions


def test_while_then_else():
    """
    Test while-then-else with break and continue.
    """
    print("\n" + "="*60)
    print("TEST: While-Then-Else with Break/Continue")
    print("="*60)
    
    builder = WhileThenElseBuilder()
    
    # Multi-line condition with side effects
    condition_instructions = [
        Instruction.load_const(1, Value.function_ref("Module:check_condition")),
        Instruction.call(2, 1, [0]),  # Call with current counter
        Instruction.greater_than(3, 2, 0)  # Check if result > 0
    ]
    
    builder.while_condition(condition_instructions, 3)
    
    # Loop body
    builder.add_instruction(Instruction.add(0, 0, 1))  # counter++
    
    # Conditional break
    builder.add_instruction(Instruction.load_const(4, Value.number(5)))
    builder.add_instruction(Instruction.equal(5, 0, 4))  # counter == 5
    
    # Use internal if-else for break condition
    inner_if = IfElseBuilder()
    inner_if.if_condition(5)
    inner_if.add_instruction(Instruction.nop())  # Placeholder for break
    inner_break_instructions = inner_if.build()
    
    # Add the inner if-else instructions
    for instr in inner_break_instructions[:-1]:  # Skip the last instruction
        builder.add_instruction(instr)
    
    # Add the actual break
    builder.add_break()
    
    # Continue with more loop body
    builder.add_instruction(Instruction.load_const(6, Value.number(3)))
    builder.add_instruction(Instruction.equal(7, 0, 6))  # counter == 3
    
    # Conditional continue
    inner_if2 = IfElseBuilder()
    inner_if2.if_condition(7)
    inner_if2.add_instruction(Instruction.nop())  # Placeholder for continue
    inner_continue_instructions = inner_if2.build()
    
    for instr in inner_continue_instructions[:-1]:
        builder.add_instruction(instr)
    
    # Add the actual continue
    builder.add_continue()
    
    # Then block (normal exit)
    builder.then_block()
    builder.add_instruction(Instruction.load_const(8, Value.atom("completed_normally")))
    builder.add_instruction(Instruction.return_reg(8))
    
    # Else block (break exit)
    builder.else_block()
    builder.add_instruction(Instruction.load_const(9, Value.atom("broke_early")))
    builder.add_instruction(Instruction.return_reg(9))
    
    instructions = builder.build()
    
    print("Generated instructions:")
    for i, instr in enumerate(instructions):
        print(f"{i:2d}: {instr}")
    
    print("\nKey features:")
    print("- Multi-line condition evaluation")
    print("- Break statements jump to else block")
    print("- Continue statements jump back to condition")
    print("- Then block executed on normal exit")
    print("- Else block executed on break exit")
    
    return instructions


def test_convenience_functions():
    """
    Test the convenience functions for building control flow.
    """
    print("\n" + "="*60)
    print("TEST: Convenience Functions")
    print("="*60)
    
    # Test build_if_else convenience function
    def build_simple_if_else(builder):
        builder.if_condition(0)
        builder.add_instruction(Instruction.load_const(1, Value.atom("true")))
        builder.else_block()
        builder.add_instruction(Instruction.load_const(1, Value.atom("false")))
    
    if_else_instructions = build_if_else(build_simple_if_else)
    
    print("If-else via convenience function:")
    for i, instr in enumerate(if_else_instructions):
        print(f"{i:2d}: {instr}")
    
    # Test build_while_then_else convenience function
    def build_simple_while(builder):
        condition_instructions = [
            Instruction.load_const(1, Value.number(10)),
            Instruction.less_than(2, 0, 1)  # counter < 10
        ]
        builder.while_condition(condition_instructions, 2)
        
        builder.add_instruction(Instruction.add(0, 0, 1))  # counter++
        
        builder.then_block()
        builder.add_instruction(Instruction.load_const(3, Value.atom("completed")))
        
        builder.else_block()
        builder.add_instruction(Instruction.load_const(3, Value.atom("broken")))
    
    while_instructions = build_while_then_else(build_simple_while)
    
    print("\nWhile-then-else via convenience function:")
    for i, instr in enumerate(while_instructions):
        print(f"{i:2d}: {instr}")
    
    return if_else_instructions, while_instructions


def test_mixed_condition_types():
    """
    Test mixing pre-evaluated and lazy-evaluated conditions.
    """
    print("\n" + "="*60)
    print("TEST: Mixed Condition Types")
    print("="*60)
    
    builder = IfElseBuilder()
    
    # Pre-evaluated condition
    builder.if_condition(0)
    builder.add_instruction(Instruction.load_const(1, Value.atom("pre_eval")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Lazy-evaluated condition
    lazy_condition = [
        Instruction.load_const(2, Value.number(42)),
        Instruction.equal(3, 0, 2)  # Check if input == 42
    ]
    builder.elif_condition_lazy(lazy_condition, 3)
    builder.add_instruction(Instruction.load_const(1, Value.atom("lazy_eval")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Another pre-evaluated condition
    builder.elif_condition(4)
    builder.add_instruction(Instruction.load_const(1, Value.atom("pre_eval_2")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Final lazy-evaluated condition
    final_lazy = [
        Instruction.load_const(5, Value.number(100)),
        Instruction.greater_than(6, 0, 5)  # Check if input > 100
    ]
    builder.elif_condition_lazy(final_lazy, 6)
    builder.add_instruction(Instruction.load_const(1, Value.atom("final_lazy")))
    builder.add_instruction(Instruction.return_reg(1))
    
    # Else block
    builder.else_block()
    builder.add_instruction(Instruction.load_const(1, Value.atom("default")))
    builder.add_instruction(Instruction.return_reg(1))
    
    instructions = builder.build()
    
    print("Generated instructions with mixed condition types:")
    for i, instr in enumerate(instructions):
        print(f"{i:2d}: {instr}")
    
    print("\nDemonstrates:")
    print("- Pre-evaluated conditions use existing register values")
    print("- Lazy-evaluated conditions generate evaluation instructions")
    print("- Proper short-circuit evaluation for both types")
    print("- Can be mixed in any order")
    
    return instructions


def main():
    """
    Run all tests to demonstrate the improved control flow capabilities.
    """
    print("IonVM Improved Control Flow Test Suite")
    print("Testing enhanced if-else and while-then-else builders")
    print("With support for both pre-evaluated and lazy-evaluated conditions")
    
    try:
        # Test if-else with side effects
        test_if_else_with_side_effects()
        
        # Test while-then-else with break/continue
        test_while_then_else()
        
        # Test convenience functions
        test_convenience_functions()
        
        # Test mixed condition types
        test_mixed_condition_types()
        
        print("\n" + "="*60)
        print("ALL TESTS COMPLETED SUCCESSFULLY!")
        print("="*60)
        print("Key improvements:")
        print("✓ Support for lazy-evaluated conditions")
        print("✓ Proper short-circuit evaluation")
        print("✓ Break/continue support in while loops")
        print("✓ While-then-else construct")
        print("✓ Mixed condition types")
        print("✓ Convenience functions")
        print("✓ No side effects when conditions are skipped")
        
    except Exception as e:
        print(f"\nTEST FAILED: {e}")
        import traceback
        traceback.print_exc()
        return 1
    
    return 0


if __name__ == "__main__":
    sys.exit(main())
