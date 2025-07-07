"""
Binary bytecode writer for IonVM.
"""
import struct
from typing import BinaryIO, List
from .function import Function


class BytecodeWriter:
    """
    Handles writing IonVM bytecode to binary format.
    """
    
    MAGIC = b"IONBC\x01\x00\x00"
    VERSION = 1
    
    def __init__(self, stream: BinaryIO):
        self.stream = stream
    
    def write_u8(self, value: int) -> None:
        """Write an unsigned 8-bit integer."""
        self.stream.write(struct.pack('<B', value))
    
    def write_u32(self, value: int) -> None:
        """Write an unsigned 32-bit integer in little-endian format."""
        self.stream.write(struct.pack('<I', value))
    
    def write_i32(self, value: int) -> None:
        """Write a signed 32-bit integer in little-endian format."""
        self.stream.write(struct.pack('<i', value))
    
    def write_f64(self, value: float) -> None:
        """Write a 64-bit floating point number in little-endian format."""
        self.stream.write(struct.pack('<d', value))
    
    def write_string(self, s: str) -> None:
        """Write a string with length prefix."""
        data = s.encode('utf-8')
        self.write_u32(len(data))
        self.stream.write(data)
    
    def write_function(self, function: Function) -> None:
        """Write a single function to the bytecode stream."""
        # Don't write magic and version for single functions
        function.serialize(self)
    
    def write_functions(self, functions: List[Function]) -> None:
        """Write multiple functions to the bytecode stream (multi-function format)."""
        # Write magic and version
        self.stream.write(self.MAGIC)
        self.write_u32(self.VERSION)
        
        # Write number of functions
        self.write_u32(len(functions))
        
        # Write each function
        for function in functions:
            function.serialize(self)
