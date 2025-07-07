"""
IonVM Instruction types and creation utilities.
"""
from typing import List


class Instruction:
    """
    Represents an IonVM instruction that can be serialized to bytecode.
    """
    
    def __init__(self, opcode: str, *args):
        self.opcode = opcode
        self.args = args
    
    # Memory instructions
    @classmethod
    def load_const(cls, reg: int, value) -> 'Instruction':
        """Load a constant value into a register."""
        return cls("load_const", reg, value)
    
    @classmethod
    def move(cls, dst: int, src: int) -> 'Instruction':
        """Move value from src register to dst register."""
        return cls("move", dst, src)
    
    # Arithmetic instructions
    @classmethod
    def add(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Add values in registers a and b, store result in dst."""
        return cls("add", dst, a, b)
    
    @classmethod
    def sub(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Subtract register b from register a, store result in dst."""
        return cls("sub", dst, a, b)
    
    @classmethod
    def mul(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Multiply values in registers a and b, store result in dst."""
        return cls("mul", dst, a, b)
    
    @classmethod
    def div(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Divide register a by register b, store result in dst."""
        return cls("div", dst, a, b)
    
    # Comparison operations
    @classmethod
    def equal(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Equality comparison (a == b)."""
        return cls("equal", dst, a, b)
    
    @classmethod
    def not_equal(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Inequality comparison (a != b)."""
        return cls("not_equal", dst, a, b)
    
    @classmethod
    def less_than(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Less than comparison (a < b)."""
        return cls("less_than", dst, a, b)
    
    @classmethod
    def less_equal(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Less than or equal comparison (a <= b)."""
        return cls("less_equal", dst, a, b)
    
    @classmethod
    def greater_than(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Greater than comparison (a > b)."""
        return cls("greater_than", dst, a, b)
    
    @classmethod
    def greater_equal(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Greater than or equal comparison (a >= b)."""
        return cls("greater_equal", dst, a, b)
    
    # Logical operations
    @classmethod
    def logical_and(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Logical AND operation (a && b)."""
        return cls("and", dst, a, b)
    
    @classmethod
    def logical_or(cls, dst: int, a: int, b: int) -> 'Instruction':
        """Logical OR operation (a || b)."""
        return cls("or", dst, a, b)
    
    @classmethod
    def logical_not(cls, dst: int, src: int) -> 'Instruction':
        """Logical NOT operation (!src)."""
        return cls("not", dst, src)

    # Property access instructions
    @classmethod
    def get_prop(cls, dst: int, obj: int, key: int) -> 'Instruction':
        """Get property from object in obj register using key register, store in dst."""
        return cls("get_prop", dst, obj, key)
    
    @classmethod
    def set_prop(cls, obj: int, key: int, value: int) -> 'Instruction':
        """Set property on object in obj register using key and value registers."""
        return cls("set_prop", obj, key, value)
    
    # Function call instructions
    @classmethod
    def call(cls, dst: int, func: int, args: List[int]) -> 'Instruction':
        """Call function in func register with args, store result in dst."""
        return cls("call", dst, func, args)
    
    # Control flow instructions
    @classmethod
    def return_reg(cls, reg: int) -> 'Instruction':
        """Return value from register."""
        return cls("return", reg)
    
    @classmethod
    def jump(cls, offset: int) -> 'Instruction':
        """Unconditional jump by offset."""
        return cls("jump", offset)
    
    @classmethod
    def jump_if_true(cls, cond: int, offset: int) -> 'Instruction':
        """Jump by offset if condition register is true."""
        return cls("jump_if_true", cond, offset)
    
    @classmethod
    def jump_if_false(cls, cond: int, offset: int) -> 'Instruction':
        """Jump by offset if condition register is false."""
        return cls("jump_if_false", cond, offset)
    
    # Process instructions
    @classmethod
    def spawn(cls, dst: int, func: int, args: List[int]) -> 'Instruction':
        """Spawn new process with function and args, store process handle in dst."""
        return cls("spawn", dst, func, args)
    
    @classmethod
    def send(cls, process: int, message: int) -> 'Instruction':
        """Send message to process."""
        return cls("send", process, message)
    
    @classmethod
    def receive(cls, dst: int) -> 'Instruction':
        """Receive message into dst register."""
        return cls("receive", dst)
    
    @classmethod
    def link(cls, process: int) -> 'Instruction':
        """Link to process for fault tolerance."""
        return cls("link", process)
    
    # Pattern matching
    @classmethod
    def match(cls, value: int, patterns: List, jump_table: List[int]) -> 'Instruction':
        """Pattern match value against patterns with jump table."""
        return cls("match", value, patterns, jump_table)
    
    # Other instructions
    @classmethod
    def yield_instr(cls) -> 'Instruction':
        """Yield control to scheduler."""
        return cls("yield")
    
    @classmethod
    def nop(cls) -> 'Instruction':
        """No operation."""
        return cls("nop")
    
    def serialize(self, writer) -> None:
        """Serialize this instruction to binary format."""
        # Opcode mappings
        opcodes = {
            "load_const": 0x01,
            "move": 0x02,
            "add": 0x03,
            "sub": 0x04,
            "mul": 0x05,
            "div": 0x06,
            "get_prop": 0x07,
            "set_prop": 0x08,
            "call": 0x09,
            "return": 0x0A,
            "jump": 0x0B,
            "jump_if_true": 0x0C,
            "jump_if_false": 0x0D,
            "spawn": 0x0E,
            "send": 0x0F,
            "receive": 0x10,
            "link": 0x11,
            "match": 0x12,
            "yield": 0x13,
            "nop": 0x14,
            "equal": 0x15,
            "not_equal": 0x16,
            "less_than": 0x17,
            "less_equal": 0x18,
            "greater_than": 0x19,
            "greater_equal": 0x1A,
            "and": 0x1B,
            "or": 0x1C,
            "not": 0x1D,
        }
        
        if self.opcode not in opcodes:
            raise ValueError(f"Unknown opcode: {self.opcode}")
        
        writer.write_u8(opcodes[self.opcode])
        
        # Serialize arguments based on instruction type
        if self.opcode == "load_const":
            reg, value = self.args
            writer.write_u32(reg)
            value.serialize(writer)
        elif self.opcode == "move":
            dst, src = self.args
            writer.write_u32(dst)
            writer.write_u32(src)
        elif self.opcode in ["add", "sub", "mul", "div"]:
            dst, a, b = self.args
            writer.write_u32(dst)
            writer.write_u32(a)
            writer.write_u32(b)
        elif self.opcode in ["get_prop", "set_prop"]:
            if self.opcode == "get_prop":
                dst, obj, key = self.args
                writer.write_u32(dst)
                writer.write_u32(obj)
                writer.write_u32(key)
            else:  # set_prop
                obj, key, value = self.args
                writer.write_u32(obj)
                writer.write_u32(key)
                writer.write_u32(value)
        elif self.opcode == "call":
            dst, func, args = self.args
            writer.write_u32(dst)
            writer.write_u32(func)
            writer.write_u32(len(args))
            for arg in args:
                writer.write_u32(arg)
        elif self.opcode == "return":
            reg, = self.args
            writer.write_u32(reg)
        elif self.opcode == "jump":
            offset, = self.args
            writer.write_i32(offset)
        elif self.opcode in ["jump_if_true", "jump_if_false"]:
            cond, offset = self.args
            writer.write_u32(cond)
            writer.write_i32(offset)
        elif self.opcode == "spawn":
            dst, func, args = self.args
            writer.write_u32(dst)
            writer.write_u32(func)
            writer.write_u32(len(args))
            for arg in args:
                writer.write_u32(arg)
        elif self.opcode == "send":
            process, message = self.args
            writer.write_u32(process)
            writer.write_u32(message)
        elif self.opcode == "receive":
            dst, = self.args
            writer.write_u32(dst)
        elif self.opcode == "link":
            process, = self.args
            writer.write_u32(process)
        elif self.opcode in ["equal", "not_equal", "less_than", "less_equal", 
                           "greater_than", "greater_equal", "and", "or"]:
            # Three-argument comparison and logical operations
            dst, a, b = self.args
            writer.write_u32(dst)
            writer.write_u32(a)
            writer.write_u32(b)
        elif self.opcode == "not":
            # Two-argument logical NOT operation
            dst, src = self.args
            writer.write_u32(dst)
            writer.write_u32(src)
        elif self.opcode in ["yield", "nop"]:
            # No arguments
            pass
        else:
            raise ValueError(f"Unsupported instruction serialization: {self.opcode}")
    
    def __repr__(self) -> str:
        return f"Instruction({self.opcode}, {self.args})"
    
    def __eq__(self, other) -> bool:
        if not isinstance(other, Instruction):
            return False
        return self.opcode == other.opcode and self.args == other.args
