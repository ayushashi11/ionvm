#!/usr/bin/env python3
"""
Test the MakeClosure instruction and args getter in the PyO3 bindings.
"""

import sys
sys.path.insert(0, '/home/top-d/ionvm/python-ionvm')

import ionvm

# Create a simple closure instruction
closure_instr = ionvm.Instruction.make_closure(
    dst=0,
    func=1,
    scope_id="test_scope_123",
    captures=[("x", 2), ("y", 3), ("z", 4)]
)

print("Closure Instruction Test")
print("=" * 50)
print(f"Opcode: {closure_instr.opcode}")
print(f"Args: {closure_instr.args}")

# Verify each argument
args = closure_instr.args
if len(args) >= 4:
    print(f"\nDetailed Args:")
    print(f"  dst:      {args[0]} (expected: 0)")
    print(f"  func:     {args[1]} (expected: 1)")
    print(f"  scope_id: {args[2]} (expected: 'test_scope_123')")
    print(f"  captures: {args[3]}")
    print(f"    captures[0]: {args[3][0]} (expected: ('x', 2))")
    print(f"    captures[1]: {args[3][1]} (expected: ('y', 3))")
    print(f"    captures[2]: {args[3][2]} (expected: ('z', 4))")
    
    # Verify correctness
    success = (
        args[0] == 0 and
        args[1] == 1 and
        args[2] == "test_scope_123" and
        len(args[3]) == 3 and
        args[3][0] == ("x", 2) and
        args[3][1] == ("y", 3) and
        args[3][2] == ("z", 4)
    )
    
    if success:
        print("\n✓ All assertions passed!")
    else:
        print("\n✗ Some assertions failed!")
        sys.exit(1)
else:
    print(f"✗ Expected at least 4 args, got {len(args)}")
    sys.exit(1)
