"""
Control flow builders for IonVM bytecode generation.

This module provides high-level constructs for building complex control flow
without manually calculating jump offsets, similar to LLVM's basic blocks.
"""
from typing import List, Callable, Optional
from .instruction import Instruction


class Condition:
    """
    Represents a condition that can be either pre-evaluated (stored in a register)
    or lazy-evaluated (instructions that produce a result).
    """
    
    def __init__(self, condition_type: str, data):
        self.condition_type = condition_type
        self.data = data
    
    @classmethod
    def from_register(cls, reg: int) -> 'Condition':
        """Create a condition from a register containing a pre-evaluated boolean."""
        return cls("register", reg)
    
    @classmethod
    def from_instructions(cls, instructions: List[Instruction], result_reg: int) -> 'Condition':
        """
        Create a condition from instructions that evaluate to a boolean.
        
        Args:
            instructions: List of instructions that evaluate the condition
            result_reg: Register where the final boolean result is stored
        """
        return cls("instructions", {"instructions": instructions, "result_reg": result_reg})
    
    def get_result_register(self) -> int:
        """Get the register where the condition result is stored."""
        if self.condition_type == "register":
            return self.data
        elif self.condition_type == "instructions":
            return self.data["result_reg"]
        else:
            raise ValueError(f"Unknown condition type: {self.condition_type}")
    
    def get_instructions(self) -> List[Instruction]:
        """Get the instructions needed to evaluate this condition."""
        if self.condition_type == "register":
            return []  # No instructions needed, already evaluated
        elif self.condition_type == "instructions":
            return self.data["instructions"]
        else:
            raise ValueError(f"Unknown condition type: {self.condition_type}")


class IfElseBuilder:
    """
    Simplified builder specifically for if-else chains with support for both
    pre-evaluated conditions (registers) and lazy-evaluated conditions (instructions).
    
    This properly implements short-circuit evaluation where conditions are only
    evaluated when needed, avoiding side effects when earlier conditions are true.
    
    Example with pre-evaluated conditions:
        builder = IfElseBuilder()
        
        # if (condition1) { ... }
        # else if (condition2) { ... }  
        # else { ... }
        
        builder.if_condition(condition1_reg)
        # Add instructions for first condition
        builder.add_instruction(Instruction.load_const(0, Value.number(1)))
        
        builder.elif_condition(condition2_reg)
        # Add instructions for second condition
        builder.add_instruction(Instruction.load_const(0, Value.number(2)))
        
        builder.else_block()
        # Add instructions for else case
        builder.add_instruction(Instruction.load_const(0, Value.number(3)))
        
        instructions = builder.build()
    
    Example with lazy-evaluated conditions:
        builder = IfElseBuilder()
        
        # Condition with side effects that should only be evaluated if needed
        condition_instructions = [
            Instruction.call(1, 0, []),  # Call function with side effects
            Instruction.greater_than(2, 1, 0)  # Check if result > 0
        ]
        
        builder.if_condition_lazy(condition_instructions, 2)
        builder.add_instruction(Instruction.load_const(0, Value.number(1)))
        
        builder.else_block()
        builder.add_instruction(Instruction.load_const(0, Value.number(2)))
        
        instructions = builder.build()
    """
    
    def __init__(self):
        self.conditions: List[Condition] = []  # Condition objects
        self.blocks: List[List[Instruction]] = []  # Instruction blocks
        self.else_instructions: List[Instruction] = []
        self.current_block: List[Instruction] = []
        self.has_else = False
        self.in_else = False
    
    def if_condition(self, condition_reg: int):
        """Start an if block with a pre-evaluated condition register."""
        if self.conditions:
            raise ValueError("if_condition() called when if-else chain already started. Use elif_condition() instead.")
        
        self.conditions.append(Condition.from_register(condition_reg))
        self.current_block = []
        self.blocks.append(self.current_block)
    
    def if_condition_lazy(self, condition_instructions: List[Instruction], result_reg: int):
        """Start an if block with lazy-evaluated condition instructions."""
        if self.conditions:
            raise ValueError("if_condition_lazy() called when if-else chain already started. Use elif_condition_lazy() instead.")
        
        self.conditions.append(Condition.from_instructions(condition_instructions, result_reg))
        self.current_block = []
        self.blocks.append(self.current_block)
    
    def elif_condition(self, condition_reg: int):
        """Add an else-if condition with a pre-evaluated condition register."""
        if not self.conditions:
            raise ValueError("elif_condition() called without if_condition()")
        if self.in_else:
            raise ValueError("elif_condition() called after else_block()")
        
        self.conditions.append(Condition.from_register(condition_reg))
        self.current_block = []
        self.blocks.append(self.current_block)
    
    def elif_condition_lazy(self, condition_instructions: List[Instruction], result_reg: int):
        """Add an else-if condition with lazy-evaluated condition instructions."""
        if not self.conditions:
            raise ValueError("elif_condition_lazy() called without if_condition()")
        if self.in_else:
            raise ValueError("elif_condition_lazy() called after else_block()")
        
        self.conditions.append(Condition.from_instructions(condition_instructions, result_reg))
        self.current_block = []
        self.blocks.append(self.current_block)
    
    def else_block(self):
        """Start the else block."""
        if not self.conditions:
            raise ValueError("else_block() called without if_condition()")
        if self.in_else:
            raise ValueError("else_block() called multiple times")
        
        self.in_else = True
        self.has_else = True
        self.current_block = self.else_instructions
    
    def add_instruction(self, instruction: Instruction):
        """Add an instruction to the current block."""
        if self.current_block is None:
            raise ValueError("add_instruction() called without starting a condition block")
        
        self.current_block.append(instruction)
    
    def build(self) -> List[Instruction]:
        """
        Build the if-else chain with proper jump calculations and short-circuit evaluation.
        
        The generated structure looks like:
        
        <condition1 instructions>
        jump_if_false +offset_to_condition2
        <block1 instructions>
        jump +offset_to_end
        condition2:
        <condition2 instructions>
        jump_if_false +offset_to_condition3_or_else
        <block2 instructions>
        jump +offset_to_end
        ...
        else:
        <else instructions>
        end:
        
        This ensures that conditions are only evaluated when needed, implementing
        proper short-circuit evaluation.
        """
        if not self.conditions:
            raise ValueError("No conditions added to if-else chain")
        
        instructions = []
        
        # We'll build this in two passes:
        # 1. Generate instructions with placeholder offsets
        # 2. Calculate and patch the actual offsets
        
        jump_patches = []  # List of (instruction_index, target_type, target_index)
        condition_starts = []  # Track where each condition actually starts
        
        # Generate the condition checks and blocks
        for i, (condition, block) in enumerate(zip(self.conditions, self.blocks)):
            # Add condition evaluation instructions (only if needed)
            condition_instructions = condition.get_instructions()
            instructions.extend(condition_instructions)
            
            # Add condition test
            condition_reg = condition.get_result_register()
            condition_jump = Instruction.jump_if_false(condition_reg, 0)  # Placeholder offset
            instructions.append(condition_jump)
            condition_jump_index = len(instructions) - 1
            
            # Add block instructions
            instructions.extend(block)
            
            # Add jump to end (except for the last block if there's no else)
            if i < len(self.conditions) - 1 or self.has_else:
                end_jump = Instruction.jump(0)  # Placeholder offset
                instructions.append(end_jump)
                jump_patches.append((len(instructions), 'end', None))
            
            # Record where this condition should jump if false
            if i < len(self.conditions) - 1:
                # Jump to next condition - we'll calculate this after all conditions are generated
                jump_patches.append((condition_jump_index, 'next_condition', i + 1))
            else:
                # Jump to else block or end
                if self.has_else:
                    jump_patches.append((condition_jump_index, 'else', None))
                else:
                    jump_patches.append((condition_jump_index, 'end', None))
        
        # Add else block if present
        else_start = len(instructions) if self.has_else else None
        if self.has_else:
            instructions.extend(self.else_instructions)
        
        # Calculate jump targets
        end_address = len(instructions)
        print(f"End address: {end_address}")
        
        # Now we need to find where each condition starts by scanning through the instructions
        # We'll rebuild the condition start positions by walking through the instruction stream
        condition_starts = []
        instruction_index = 0
        
        for i, (condition, block) in enumerate(zip(self.conditions, self.blocks)):
            # This condition starts at the current instruction index
            condition_starts.append(instruction_index)
            
            # Skip over condition evaluation instructions
            condition_instructions = condition.get_instructions()
            instruction_index += len(condition_instructions)
            
            # Skip over the condition test jump (only 1 instruction)
            instruction_index += 1
            
            # Skip over the block instructions
            instruction_index += len(block)
            
            # Skip over the jump to end (if present)
            if i < len(self.conditions) - 1 or self.has_else:
                instruction_index += 1
        
        # Patch the jumps
        for patch_index, target_type, target_index in jump_patches:
            if target_type == 'end':
                offset = end_address - patch_index - (1 if self.has_else else 0)
            elif target_type == 'next_condition':
                offset = condition_starts[target_index] - patch_index
            elif target_type == 'else':
                offset = else_start - patch_index
            else:
                raise ValueError(f"Unknown target type: {target_type}")
            print(f"Patch {patch_index} to {target_type} with offset {offset}")
            
            # Update the instruction's offset
            instruction = instructions[patch_index]
            if instruction.opcode == 'jump_if_false':
                instructions[patch_index] = Instruction.jump_if_false(instruction.args[0], offset)
            elif instruction.opcode == 'jump':
                instructions[patch_index] = Instruction.jump(offset)
        
        return instructions


class BasicBlock:
    """
    Represents a basic block of instructions with a label.
    Similar to LLVM basic blocks but for IonVM bytecode.
    """
    
    def __init__(self, name: str):
        self.name = name
        self.instructions: List[Instruction] = []
        self.start_address: Optional[int] = None
        self.end_address: Optional[int] = None
    
    def add_instruction(self, instruction: Instruction):
        """Add an instruction to this basic block."""
        self.instructions.append(instruction)
    
    def __len__(self):
        return len(self.instructions)


class BreakContinueException(Exception):
    """Exception used internally to track break/continue statements."""
    
    def __init__(self, break_type: str, target_label: Optional[str] = None):
        self.break_type = break_type  # 'break' or 'continue'
        self.target_label = target_label
        super().__init__(f"{break_type} statement")


class WhileThenElseBuilder:
    """
    Builder for while loops with optional then and else blocks.
    
    The while-then-else construct works as follows:
    - while: The loop condition and body (supports multi-line conditions)
    - then: Executed when the loop exits normally (condition becomes false)
    - else: Executed when the loop exits via break
    
    Example:
        builder = WhileThenElseBuilder()
        
        # Multi-line condition evaluation
        condition_instructions = [
            Instruction.call(1, 0, []),  # Call function that might have side effects
            Instruction.greater_than(2, 1, 0)  # Check if result > 0
        ]
        
        builder.while_condition(condition_instructions, 2)
        # Add instructions for while body
        builder.add_instruction(Instruction.load_const(0, Value.number(1)))
        builder.add_break()  # This will jump to else block
        
        builder.then_block()
        # Add instructions for then block (normal exit)
        builder.add_instruction(Instruction.load_const(0, Value.number(2)))
        
        builder.else_block()
        # Add instructions for else block (break exit)
        builder.add_instruction(Instruction.load_const(0, Value.number(3)))
        
        instructions = builder.build()
    """
    
    def __init__(self):
        self.condition: Optional[Condition] = None
        self.while_instructions: List[Instruction] = []
        self.then_instructions: List[Instruction] = []
        self.else_instructions: List[Instruction] = []
        self.current_block: Optional[List[Instruction]] = None
        self.in_then = False
        self.in_else = False
        self.break_points: List[int] = []  # Instruction indices where breaks occur
        self.continue_points: List[int] = []  # Instruction indices where continues occur
    
    def while_condition(self, condition_instructions: List[Instruction], result_reg: int):
        """Start a while loop with multi-line condition evaluation."""
        if self.condition is not None:
            raise ValueError("while_condition() called when while loop already started")
        
        self.condition = Condition.from_instructions(condition_instructions, result_reg)
        self.current_block = self.while_instructions
    
    def while_condition_reg(self, condition_reg: int):
        """Start a while loop with a pre-evaluated condition register."""
        if self.condition is not None:
            raise ValueError("while_condition_reg() called when while loop already started")
        
        self.condition = Condition.from_register(condition_reg)
        self.current_block = self.while_instructions
    
    def add_instruction(self, instruction: Instruction):
        """Add an instruction to the current block."""
        if self.current_block is None:
            raise ValueError("add_instruction() called without starting a while loop")
        
        self.current_block.append(instruction)
    
    def add_break(self):
        """Add a break statement that will jump to the else block."""
        if self.current_block is None:
            raise ValueError("add_break() called without starting a while loop")
        if self.in_then or self.in_else:
            raise ValueError("add_break() called outside of while loop body")
        
        # Add a placeholder jump instruction
        break_jump = Instruction.jump(0)  # Placeholder offset
        self.current_block.append(break_jump)
        self.break_points.append(len(self.while_instructions) - 1)
    
    def add_continue(self):
        """Add a continue statement that will jump back to the condition check."""
        if self.current_block is None:
            raise ValueError("add_continue() called without starting a while loop")
        if self.in_then or self.in_else:
            raise ValueError("add_continue() called outside of while loop body")
        
        # Add a placeholder jump instruction
        continue_jump = Instruction.jump(0)  # Placeholder offset
        self.current_block.append(continue_jump)
        self.continue_points.append(len(self.while_instructions) - 1)
    
    def then_block(self):
        """Start the then block (executed on normal loop exit)."""
        if self.condition is None:
            raise ValueError("then_block() called without while_condition()")
        if self.in_then:
            raise ValueError("then_block() called multiple times")
        
        self.in_then = True
        self.current_block = self.then_instructions
    
    def else_block(self):
        """Start the else block (executed on break exit)."""
        if self.condition is None:
            raise ValueError("else_block() called without while_condition()")
        if self.in_else:
            raise ValueError("else_block() called multiple times")
        
        self.in_else = True
        self.current_block = self.else_instructions
    
    def build(self) -> List[Instruction]:
        """
        Build the while loop with proper jump calculations.
        
        The generated structure looks like:
        
        start_while:
            <condition instructions>
            jump_if_false then_block
            <while body instructions>
            jump start_while
        then_block:
            <then instructions>
            jump end
        else_block:
            <else instructions>
        end:
        
        Break statements jump to else_block
        Continue statements jump to start_while
        """
        if self.condition is None:
            raise ValueError("No condition added to while loop")
        
        #check for breaks and continues and add them to the lists:
        for i,instr in enumerate(self.while_instructions):
            if instr.opcode == 'break':
                self.break_points.append(i)
            elif instr.opcode == 'continue':
                self.continue_points.append(i)
        
        instructions = []
        
        # Record where loop starts (for continue statements)
        loop_start = len(instructions)
        
        # Add condition evaluation instructions
        condition_instructions = self.condition.get_instructions()
        instructions.extend(condition_instructions)
        
        # Add condition test - jump to then block if false
        condition_reg = self.condition.get_result_register()
        condition_jump = Instruction.jump_if_false(condition_reg, 0)  # Placeholder offset
        instructions.append(condition_jump)
        condition_jump_index = len(instructions) - 1
        
        # Add while body instructions
        body_start = len(instructions)
        instructions.extend(self.while_instructions)
        
        # Add jump back to loop start
        loop_back_offset = loop_start - len(instructions) 
        instructions.append(Instruction.jump(loop_back_offset))
        
        # Then block starts here
        then_start = len(instructions)
        instructions.extend(self.then_instructions)
        
        # Jump to end (skip else block)
        if self.else_instructions:
            then_end_jump = Instruction.jump(0)  # Placeholder offset
            instructions.append(then_end_jump)
            then_end_jump_index = len(instructions) - 1
        
        # Else block starts here
        else_start = len(instructions)
        instructions.extend(self.else_instructions)
        
        # End of entire construct
        end_address = len(instructions)
        
        # Patch the condition jump to point to then block
        condition_offset = then_start - condition_jump_index
        instructions[condition_jump_index] = Instruction.jump_if_false(condition_reg, condition_offset)
        
        # Patch the then block end jump to point to end
        if self.else_instructions:
            then_end_offset = end_address - then_end_jump_index 
            instructions[then_end_jump_index] = Instruction.jump(then_end_offset)
        
        # Patch break statements to jump to else block
        for break_point in self.break_points:
            break_instruction_index = body_start + break_point
            break_offset = else_start - break_instruction_index 
            instructions[break_instruction_index] = Instruction.jump(break_offset)
        
        # Patch continue statements to jump to loop start
        for continue_point in self.continue_points:
            continue_instruction_index = body_start + continue_point
            continue_offset = loop_start - continue_instruction_index 
            instructions[continue_instruction_index] = Instruction.jump(continue_offset)
        
        return instructions


# Convenience functions
def build_if_else(builder_func: Callable[[IfElseBuilder], None]) -> List[Instruction]:
    """
    Convenience function for building if-else chains.
    
    Example:
        def build_logic(builder):
            builder.if_condition(condition1_reg)
            builder.add_instruction(Instruction.load_const(0, Value.number(1)))
            builder.elif_condition(condition2_reg)
            builder.add_instruction(Instruction.load_const(0, Value.number(2)))
            builder.else_block()
            builder.add_instruction(Instruction.load_const(0, Value.number(3)))
        
        instructions = build_if_else(build_logic)
    """
    builder = IfElseBuilder()
    builder_func(builder)
    return builder.build()


def build_while_then_else(builder_func: Callable[[WhileThenElseBuilder], None]) -> List[Instruction]:
    """
    Convenience function for building while-then-else loops.
    
    Example:
        def build_loop(builder):
            condition_instructions = [
                Instruction.load_const(1, Value.number(10)),
                Instruction.less_than(2, 0, 1)  # counter < 10
            ]
            builder.while_condition(condition_instructions, 2)
            
            # Loop body
            builder.add_instruction(Instruction.add(0, 0, 1))  # counter++
            
            builder.then_block()
            builder.add_instruction(Instruction.load_const(3, Value.atom("completed")))
            
            builder.else_block()
            builder.add_instruction(Instruction.load_const(3, Value.atom("broken")))
        
        instructions = build_while_then_else(build_loop)
    """
    builder = WhileThenElseBuilder()
    builder_func(builder)
    return builder.build()


def create_break_instruction() -> Instruction:
    """
    Create a break instruction for use in while loops.
    Note: This is a placeholder that should be used with WhileThenElseBuilder.add_break()
    """
    return Instruction.jump(0)  # Placeholder - will be patched by builder


def create_continue_instruction() -> Instruction:
    """
    Create a continue instruction for use in while loops.
    Note: This is a placeholder that should be used with WhileThenElseBuilder.add_continue()
    """
    return Instruction.jump(0)  # Placeholder - will be patched by builder
        