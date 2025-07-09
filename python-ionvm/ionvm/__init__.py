"""
IonVM Python Library

Python library for creating IonVM bytecode and IonPack files.
"""

from .value import Value
from .instruction import Instruction
from .function import Function
from .bytecode import BytecodeWriter
from .ionpack import IonPackBuilder, Manifest
from .control_flow import (
    IfElseBuilder, 
    WhileThenElseBuilder,
    build_if_else, 
    build_while_then_else,
    create_break_instruction,
    create_continue_instruction
)

__version__ = "0.1.0"
__all__ = [
    "Value",
    "Instruction", 
    "Function",
    "BytecodeWriter",
    "IonPackBuilder",
    "Manifest",
    "IfElseBuilder",
    "WhileThenElseBuilder",
    "build_if_else",
    "build_while_then_else",
    "create_break_instruction",
    "create_continue_instruction",
]
