#!/usr/bin/env python3
"""
Create an IonPack file demonstrating the improved control flow features
and run it with the IonVM to show the functionality in action.
"""

import sys
import os
import subprocess

# Add the python-ionvm directory to the path
sys.path.append(os.path.join(os.path.dirname(__file__), 'python-ionvm'))

from ionvm import (
    IonPackBuilder, 
    Function, 
    Instruction, 
    Value,
    IfElseBuilder,
    WhileThenElseBuilder
)


def create_side_effect_function():
    """
    Create a function that has side effects - prints a message and returns a value.
    This demonstrates why lazy evaluation is important.
    """
    instructions = [
        # Print a message to show this function was called
        Instruction.load_const(0, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(1, Value.string("Side effect function called!")),
        Instruction.call(2, 0, [1]),
        
        # Return a value that can be used in conditions
        Instruction.load_const(3, Value.number(42)),
        Instruction.return_reg(3)
    ]
    
    return Function(
        name="side_effect_func",
        arity=0,
        extra_regs=4,
        instructions=instructions
    )


def create_condition_checker():
    """
    Create a function that checks a condition and has side effects.
    """
    instructions = [
        # Print that we're checking the condition
        Instruction.load_const(1, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(2, Value.string("Checking condition...")),
        Instruction.call(3, 1, [2]),
        
        # Get the input parameter (register 0 after function call setup)
        # Check if it's greater than 5
        Instruction.load_const(4, Value.number(5)),
        Instruction.greater_than(5, 0, 4),
        
        # Return the result
        Instruction.return_reg(5)
    ]
    
    return Function(
        name="condition_checker",
        arity=1,
        extra_regs=6,
        instructions=instructions
    )


def create_if_else_demo():
    """
    Create a function demonstrating if-else with lazy evaluation.
    """
    init = [
        Instruction.load_const(1, Value.atom("__stdlib:PrintLn")),
        Instruction.call(1, 1, [0])# r0 will hold the input
    ]
    builder = IfElseBuilder()
    
    # First condition: check if input (r0) equals 1
    builder.if_condition_lazy([
        Instruction.load_const(1, Value.number(1)),
        Instruction.equal(2, 0, 1)
    ], 2)
    
    # First block - no side effects needed
    builder.add_instruction(Instruction.load_const(8, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(1, Value.string("Input was 1")))
    builder.add_instruction(Instruction.call(2, 8, [1]))
    builder.add_instruction(Instruction.load_const(3, Value.string("first")))
    builder.add_instruction(Instruction.return_reg(3))
    
    # Second condition: call side effect function only if first condition fails
    builder.elif_condition_lazy([
        Instruction.load_const(3, Value.atom("__function_ref:SideEffect:side_effect_func")),
        Instruction.call(4, 3, []),
        Instruction.equal(5, 0, 4)  # Check if input equals the returned value (42)
    ], 5)
    
    # Second block
    builder.add_instruction(Instruction.load_const(8, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(1, Value.string("Input matched side effect result")))
    builder.add_instruction(Instruction.call(2, 8, [1]))
    builder.add_instruction(Instruction.load_const(3, Value.string("second")))
    builder.add_instruction(Instruction.return_reg(3))
    
    # Third condition: another side effect function
    builder.elif_condition_lazy([
        Instruction.load_const(6, Value.atom("__function_ref:ConditionChecker:condition_checker")),
        Instruction.call(7, 6, [0]),  # Pass input as parameter
    ], 7)
    
    # Third block
    builder.add_instruction(Instruction.load_const(8, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(1, Value.string("Condition checker returned true")))
    builder.add_instruction(Instruction.call(2, 8, [1]))
    builder.add_instruction(Instruction.load_const(3, Value.string("third")))
    builder.add_instruction(Instruction.return_reg(3))
    
    # Else block
    builder.else_block()
    builder.add_instruction(Instruction.load_const(8, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(1, Value.string("No conditions matched")))
    builder.add_instruction(Instruction.call(2, 8, [1]))
    builder.add_instruction(Instruction.load_const(3, Value.string("default")))
    builder.add_instruction(Instruction.return_reg(3))
    
    instructions = init+builder.build()
    return Function(
        name="if_else_demo",
        arity=1,
        extra_regs=9,
        instructions=instructions
    )


def create_while_then_else_demo():
    """
    Create a function demonstrating while-then-else with break/continue.
    """
    builder = WhileThenElseBuilder()
    
    # Initialize counter to 0
    instructions = [
        Instruction.load_const(0, Value.number(0)),  # counter = 0
    ]
    
    # While condition: counter < 10, but with a side effect checker
    builder.while_condition([
        Instruction.load_const(1, Value.atom("__function_ref:ConditionChecker:condition_checker")),
        Instruction.call(2, 1, [0]),  # Check condition with current counter
        Instruction.load_const(3, Value.number(10)),
        Instruction.less_than(4, 0, 3),  # counter < 10
        Instruction.logical_or(5, 2, 4)  # condition_checker result or counter < 10
    ], 5)
    
    # Loop body
    builder.add_instruction(Instruction.load_const(6, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(7, Value.string("Loop iteration")))
    builder.add_instruction(Instruction.call(8, 6, [7]))
    
    # Increment counter
    builder.add_instruction(Instruction.load_const(9, Value.number(1)))
    builder.add_instruction(Instruction.add(0, 0, 9))  # counter++
    
    # Break condition: if counter == 5, break early
    builder.add_instruction(Instruction.load_const(10, Value.number(5)))
    builder.add_instruction(Instruction.equal(11, 0, 10))
    builder.add_instruction(Instruction.load_const(12, Value.atom("__stdlib:PrintLn")))
    
    # Create inner if for break
    inner_if = IfElseBuilder()
    inner_if.if_condition(11)
    inner_if.add_instruction(Instruction.load_const(13, Value.string("Breaking early at 5")))
    inner_if.add_instruction(Instruction.call(14, 12, [13]))
    inner_if.add_instruction(Instruction.break_instr())  # Break out of the loop
    inner_if.else_block()
    inner_if.add_instruction(Instruction.load_const(13, Value.string("Not breaking early at 5")))
    inner_if.add_instruction(Instruction.call(14, 12, [13]))
    inner_break_instructions = inner_if.build()
    
    # Add inner if instructions (without the final jump)
    for instr in inner_break_instructions:
        print(f"{instr}")
        builder.add_instruction(instr)
    
    # Continue condition: if counter == 3, continue (skip rest of loop)
    builder.add_instruction(Instruction.load_const(15, Value.number(3)))
    builder.add_instruction(Instruction.equal(16, 0, 15))
    
    # Create inner if for continue
    inner_if2 = IfElseBuilder()
    inner_if2.if_condition(16)
    inner_if2.add_instruction(Instruction.load_const(17, Value.atom("__stdlib:PrintLn")))
    inner_if2.add_instruction(Instruction.load_const(18, Value.string("Continuing at 3")))
    inner_if2.add_instruction(Instruction.call(19, 17, [18]))
    inner_if2.add_instruction(Instruction.continue_instr())  # Continue to next iteration
    inner_continue_instructions = inner_if2.build()
    
    # Add inner if instructions (without the final jump)
    for instr in inner_continue_instructions:
        builder.add_instruction(instr)
    
    # Rest of loop body
    builder.add_instruction(Instruction.load_const(20, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(21, Value.string("End of loop body")))
    builder.add_instruction(Instruction.call(22, 20, [21]))
    
    # Then block (normal exit)
    builder.then_block()
    builder.add_instruction(Instruction.load_const(23, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(24, Value.string("Loop completed normally")))
    builder.add_instruction(Instruction.call(25, 23, [24]))
    builder.add_instruction(Instruction.load_const(26, Value.string("completed")))
    builder.add_instruction(Instruction.return_reg(26))
    
    # Else block (break exit)
    builder.else_block()
    builder.add_instruction(Instruction.load_const(27, Value.atom("__stdlib:PrintLn")))
    builder.add_instruction(Instruction.load_const(28, Value.string("Loop was broken early")))
    builder.add_instruction(Instruction.call(29, 27, [28]))
    builder.add_instruction(Instruction.load_const(30, Value.string("broken")))
    builder.add_instruction(Instruction.return_reg(30))
    
    loop_instructions = builder.build()
    
    # Combine initialization with loop
    all_instructions = instructions + loop_instructions
    return Function(
        name="while_then_else_demo",
        arity=0,
        extra_regs=31,
        instructions=all_instructions
    )


def create_main_function():
    """
    Create the main function that tests different scenarios.
    """
    instructions = [
        # Test 1: if-else with input = 1 (should hit first condition, no side effects)
        Instruction.load_const(0, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(1, Value.string("=== Test 1: Input = 1 (should hit first condition) ===")),
        Instruction.call(2, 0, [1]),
        
        Instruction.load_const(3, Value.number(1)),
        Instruction.load_const(4, Value.atom("__function_ref:IfElseDemo:if_else_demo")),
        Instruction.call(5, 4, [3]),
        
        # Test 2: if-else with input = 42 (should hit second condition with side effects)
        Instruction.load_const(6, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(7, Value.string("=== Test 2: Input = 42 (should hit second condition) ===")),
        Instruction.call(8, 6, [7]),
        
        Instruction.load_const(9, Value.number(42)),
        Instruction.load_const(10, Value.atom("__function_ref:IfElseDemo:if_else_demo")),
        Instruction.call(11, 10, [9]),
        
        # Test 3: if-else with input = 7 (should hit third condition with side effects)
        Instruction.load_const(12, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(13, Value.string("=== Test 3: Input = 7 (should hit third condition) ===")),
        Instruction.call(14, 12, [13]),
        
        Instruction.load_const(15, Value.number(7)),
        Instruction.load_const(16, Value.atom("__function_ref:IfElseDemo:if_else_demo")),
        Instruction.call(17, 16, [15]),
        
        # Test 4: if-else with input = 2 (should hit else block)
        Instruction.load_const(18, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(19, Value.string("=== Test 4: Input = 2 (should hit else block) ===")),
        Instruction.call(20, 18, [19]),
        
        Instruction.load_const(21, Value.number(2)),
        Instruction.load_const(22, Value.atom("__function_ref:IfElseDemo:if_else_demo")),
        Instruction.call(23, 22, [21]),
        
        # Test 5: while-then-else demo
        Instruction.load_const(24, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(25, Value.string("=== Test 5: While-then-else demo ===")),
        Instruction.call(26, 24, [25]),
        
        Instruction.load_const(27, Value.atom("__function_ref:WhileDemo:while_then_else_demo")),
        Instruction.call(28, 27, []),
        
        # Final message
        Instruction.load_const(29, Value.atom("__stdlib:PrintLn")),
        Instruction.load_const(30, Value.string("All tests completed!")),
        Instruction.call(31, 29, [30]),
        Instruction.return_reg(31)
    ]
    
    return Function(
        name="main",
        arity=0,
        extra_regs=32,
        instructions=instructions
    )


def create_ionpack():
    """
    Create the IonPack file with all the demonstration functions.
    """
    print("Creating IonPack with improved control flow demo...")
    
    builder = IonPackBuilder("improved-control-flow", "1.0.0")
    builder.main_class("Main")
    builder.entry_point("main")
    builder.description("Demonstrates improved control flow with lazy evaluation")
    builder.author("IonVM Control Flow Demo")
    
    # Add all functions as classes
    builder.add_class("SideEffect", create_side_effect_function())
    builder.add_class("ConditionChecker", create_condition_checker())
    builder.add_class("IfElseDemo", create_if_else_demo())
    builder.add_class("WhileDemo", create_while_then_else_demo())
    builder.add_class("Main", create_main_function())
    
    # Add source code for reference
    builder.add_source("demo.ion", """
// Improved control flow demo
// Features:
// - Lazy evaluation of conditions
// - Short-circuit evaluation
// - While-then-else constructs
// - Break and continue support

function main() {
    // This is equivalent to the generated bytecode
    return "demo";
}
""")
    
    # Build the package
    output_file = "improved_control_flow_demo.ionpack"
    with open(output_file, "wb") as f:
        builder.build(f)
    
    print(f"IonPack created: {output_file}")
    return output_file


def run_ionpack(ionpack_file):
    """
    Run the IonPack file using the IonVM.
    """
    print(f"Running {ionpack_file} with IonVM...")
    print("=" * 60)
    
    try:
        # Run the IonVM with the created IonPack with debug flag
        result = subprocess.run([
            'cargo', 'run', '--bin', 'ionvm', 'run', '-d', ionpack_file
        ], capture_output=True, text=True, cwd='/home/top-d/vmm', check=False, timeout=15)
        
        print("STDOUT:")
        print(result.stdout)
        
        if result.stderr:
            print("STDERR:")
            print(result.stderr)
        
        print(f"Exit code: {result.returncode}")
        
    except Exception as e:
        print(f"Error running IonVM: {e}")
        return False
    
    return result.returncode == 0


def main():
    """
    Main function to create and run the demo.
    """
    print("IonVM Improved Control Flow Demo")
    print("Creating IonPack with lazy evaluation and while-then-else...")
    
    try:
        # Create the IonPack
        ionpack_file = create_ionpack()
        
        # Run it
        success = run_ionpack(ionpack_file)
        
        if success:
            print("\n" + "=" * 60)
            print("DEMO COMPLETED SUCCESSFULLY!")
            print("=" * 60)
            print("Key features demonstrated:")
            print("✓ Lazy evaluation prevents unnecessary side effects")
            print("✓ Short-circuit evaluation in if-else chains")
            print("✓ While-then-else with break/continue")
            print("✓ Proper jump calculations")
            print("✓ Mixed condition types")
        else:
            print("Demo failed to run properly.")
            return 1
            
    except Exception as e:
        print(f"Error: {e}")
        import traceback
        traceback.print_exc()
        return 1
    
    return 0


if __name__ == "__main__":
    sys.exit(main())
