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
    def load_const(cls, reg: float, value) -> 'Instruction':
        """Load a constant value into a register."""
        return cls("load_const", reg, value)
    
    @classmethod
    def object_init(cls, dst: float, kvs: list) -> 'Instruction':
        """
        Object initialization with mixed register/value arguments and property flags.
        kvs: list of (key, arg), where arg is:
            ('reg', regnum, flags_dict) or ('val', Value, flags_dict)
        flags_dict: {'writable': bool, 'enumerable': bool, 'configurable': bool} (all default True if omitted)
        For backward compatibility, ('reg', regnum) or ('val', Value) is allowed (all flags True).
        """
        return cls("object_init", dst, kvs)
    
    @classmethod
    def move(cls, dst: float, src: float) -> 'Instruction':
        """Move value from src register to dst register."""
        return cls("move", dst, src)
    
    # Arithmetic instructions
    @classmethod
    def add(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Add values in registers a and b, store result in dst."""
        return cls("add", dst, a, b)
    
    @classmethod
    def sub(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Subtract register b from register a, store result in dst."""
        return cls("sub", dst, a, b)
    
    @classmethod
    def mul(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Multiply values in registers a and b, store result in dst."""
        return cls("mul", dst, a, b)
    
    @classmethod
    def div(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Divide register a by register b, store result in dst."""
        return cls("div", dst, a, b)
    
    # Comparison operations
    @classmethod
    def equal(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Equality comparison (a == b)."""
        return cls("equal", dst, a, b)
    
    @classmethod
    def not_equal(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Inequality comparison (a != b)."""
        return cls("not_equal", dst, a, b)
    
    @classmethod
    def less_than(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Less than comparison (a < b)."""
        return cls("less_than", dst, a, b)
    
    @classmethod
    def less_equal(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Less than or equal comparison (a <= b)."""
        return cls("less_equal", dst, a, b)
    
    @classmethod
    def greater_than(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Greater than comparison (a > b)."""
        return cls("greater_than", dst, a, b)
    
    @classmethod
    def greater_equal(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Greater than or equal comparison (a >= b)."""
        return cls("greater_equal", dst, a, b)
    
    # Logical operations
    @classmethod
    def logical_and(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Logical AND operation (a && b)."""
        return cls("and", dst, a, b)
    
    @classmethod
    def logical_or(cls, dst: float, a: float, b: float) -> 'Instruction':
        """Logical OR operation (a || b)."""
        return cls("or", dst, a, b)
    
    @classmethod
    def logical_not(cls, dst: float, src: float) -> 'Instruction':
        """Logical NOT operation (!src)."""
        return cls("not", dst, src)

    # Property access instructions
    @classmethod
    def get_prop(cls, dst: float, obj: float, key: float) -> 'Instruction':
        """Get property from object in obj register using key register, store in dst."""
        return cls("get_prop", dst, obj, key)
    
    @classmethod
    def set_prop(cls, obj: float, key: float, value: float) -> 'Instruction':
        """Set property on object in obj register using key and value registers."""
        return cls("set_prop", obj, key, value)
    
    # Function call instructions
    @classmethod
    def call(cls, dst: float, func: float, args: List[float]) -> 'Instruction':
        """Call function in func register with args, store result in dst."""
        return cls("call", dst, func, args)
    
    # Control flow instructions
    @classmethod
    def return_reg(cls, reg: float) -> 'Instruction':
        """Return value from register."""
        return cls("return", reg)
    
    @classmethod
    def jump(cls, offset: float) -> 'Instruction':
        """Unconditional jump by offset."""
        return cls("jump", offset)
    
    @classmethod
    def jump_if_true(cls, cond: float, offset: float) -> 'Instruction':
        """Jump by offset if condition register is true."""
        return cls("jump_if_true", cond, offset)
    
    @classmethod
    def jump_if_false(cls, cond: float, offset: float) -> 'Instruction':
        """Jump by offset if condition register is false."""
        return cls("jump_if_false", cond, offset)
    
    # Process instructions
    @classmethod
    def spawn(cls, dst: float, func: float, args: List[float]) -> 'Instruction':
        """Spawn new process with function and args, store process handle in dst."""
        return cls("spawn", dst, func, args)
    
    @classmethod
    def send(cls, process: float, message: float) -> 'Instruction':
        """Send message to process."""
        return cls("send", process, message)
    
    @classmethod
    def receive(cls, dst: float) -> 'Instruction':
        """Receive message into dst register."""
        return cls("receive", dst)
    
    @classmethod
    def receive_with_timeout(cls, dst: float, timeout: float, result: float) -> 'Instruction':
        """Receive message into dst register with timeout, store result in result register."""
        return cls("receive_with_timeout", dst, timeout, result)
    
    @classmethod
    def link(cls, process: float) -> 'Instruction':
        """Link to process for fault tolerance."""
        return cls("link", process)
    
    # Pattern matching
    @classmethod
    def match(cls, value: float, patterns: list, jump_table: list) -> 'Instruction':
        """
        Pattern match value against patterns with jump table.
        patterns: list of Pattern objects (see pattern.py)
        jump_table: list of jump offsets (ints)
        """
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
    
    @classmethod
    def break_instr(cls) -> 'Instruction':
        """Break out of a loop or control structure.(Placeholder opcode, shouldnt appear in final code)"""
        return cls("break")

    @classmethod
    def continue_instr(cls) -> 'Instruction':
        """Continue to the next iteration of a loop.(Placeholder opcode, shouldnt appear in final code)"""
        return cls("continue")
    
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
            "receive_with_timeout": 0x1E,
            "object_init": 0x1F,
        }
        
        if self.opcode not in opcodes:
            raise ValueError(f"Unknown opcode: {self.opcode}")
        
        writer.write_u8(opcodes[self.opcode])
        
        # Serialize arguments based on instruction type
        if self.opcode == "match":
            # args: value_reg, patterns (list of Pattern), jump_table (list of offsets)
            value_reg, patterns, jump_table = self.args
            writer.write_u32(int(value_reg))
            writer.write_u32(len(patterns))
            for pat, offset in zip(patterns, jump_table):
                pat.serialize(writer)
                writer.write_i32(int(offset))
            return
        if self.opcode == "object_init":
            dst, kvs = self.args
            writer.write_u32(dst)
            writer.write_u32(len(kvs))
            for key, arg in kvs:
                writer.write_string(key)
                # Normalize arg to (kind, value, flags)
                if len(arg) == 2:
                    kind, value = arg
                    flags = {'writeable': True, 'enumerable': False, 'configurable': True}
                elif len(arg) == 3:
                    kind, value, flags = arg
                    # Fill missing flags with True
                    flags = {k: flags.get(k, k!='enumerable') for k in ['writeable', 'enumerable', 'configurable']}
                else:
                    raise ValueError(f"Invalid ObjectInitArg: {arg}")
                if kind == 'reg':
                    writer.write_u8(2)
                    writer.write_u32(value)
                elif kind == 'val':
                    writer.write_u8(3)
                    value.serialize(writer)
                else:
                    raise ValueError(f"Invalid ObjectInitArg kind: {kind}")
                # Write flags (as 3 bytes: 1=on, 0=off)
                writer.write_u8(1 if flags['writeable'] else 0)
                writer.write_u8(1 if flags['enumerable'] else 0)
                writer.write_u8(1 if flags['configurable'] else 0)
            return
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
        elif self.opcode == "receive_with_timeout":
            dst, timeout, result = self.args
            writer.write_u32(dst)
            writer.write_u32(timeout)
            writer.write_u32(result)
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
