"""
IonVM Function representation and serialization.
"""
from typing import List, Optional
from .instruction import Instruction


class Function:
    """
    Represents an IonVM function with its bytecode instructions.
    """
    
    def __init__(self, name: Optional[str], arity: int, extra_regs: int, 
                 instructions: List[Instruction]):
        self.name = name
        self.arity = arity
        self.extra_regs = extra_regs
        self.instructions = instructions
    
    def serialize(self, writer, debug=False) -> None:
        """Serialize this function to binary format."""
        # Write function name (with has_name flag)
        if self.name:
            writer.write_u8(1)  # Has name
            writer.write_string(self.name)
        else:
            writer.write_u8(0)  # No name
        # Write arity and extra_regs
        writer.write_u32(self.arity)
        writer.write_u32(self.extra_regs)
        
        # Write function type (0 = bytecode, 1 = FFI)
        writer.write_u8(0)  # Always bytecode for now
        if debug:
            print(f"Serializing function: {self.name}, arity={self.arity}, extra_regs={self.extra_regs}")
            print(f"stream is now: {writer.stream.getbuffer().hex()}")
        # Write number of instructions
        writer.write_u32(len(self.instructions))
        
        # Write each instruction
        for instruction in self.instructions:
            instruction.serialize(writer)
            if debug:
                print(f"Serializing instruction for function: {self.name}, {instruction}")
                print(f"stream is now: {writer.stream.getbuffer().hex()}")

    def __repr__(self) -> str:
        return f"Function({self.name}, arity={self.arity}, extra_regs={self.extra_regs}, {len(self.instructions)} instructions)"

class FunctionBuilder:
    """
    Builder for constructing IonVM functions.
    """
    
    def __init__(self, name: Optional[str] = None, arity: int = 0, extra_regs: int = 0):
        self.name = name
        self.arity = arity
        self.extra_regs = extra_regs
        self.instructions: List[Instruction] = []
    
    def add_instruction(self, instruction: Instruction) -> 'FunctionBuilder':
        """Add an instruction to the function."""
        self.instructions.append(instruction)
        return self
    
    def build(self) -> Function:
        """Build the Function object."""
        return Function(self.name, self.arity, self.extra_regs, self.instructions)
