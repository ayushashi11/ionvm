"""
IonVM Python Library

A Python library for creating IonVM bytecode and IonPack files programmatically.
IonVM is a research virtual machine implementing the actor model of computation
with support for prototype-based objects and preemptive scheduling.

This library enables you to:
- Generate IonVM bytecode instructions
- Create and manipulate IonVM values  
- Build IonPack archive files for distribution
- Construct control flow patterns
- Work with pattern matching

Quick Start:
    >>> from ionvm import Function, Instruction, Value, IonPackBuilder
    >>> 
    >>> # Create a simple function
    >>> function = Function(
    ...     name="main",
    ...     arity=0,
    ...     extra_regs=1,
    ...     instructions=[
    ...         Instruction.load_const(0, Value.number(42)),
    ...         Instruction.return_reg(0)
    ...     ]
    ... )
    >>> 
    >>> # Create an IonPack
    >>> builder = IonPackBuilder("hello-world", "1.0.0")
    >>> builder.main_class("Main")
    >>> builder.entry_point("main")
    >>> builder.add_class("Main", function)
    >>> 
    >>> with open("hello.ionpack", "wb") as f:
    ...     builder.build(f)

Architecture:
    The library mirrors the structure of the IonVM:
    
    - Values: Represent IonVM data types (numbers, strings, objects, etc.)
    - Instructions: VM operations (arithmetic, control flow, actor operations)
    - Functions: Collections of instructions with metadata
    - IonPack: Archive format for distributing programs
    - Patterns: Structural pattern matching support
"""


from .value import Value
from .instruction import Instruction
from .function import Function
from .bytecode import BytecodeWriter
from .ionpack import IonPackBuilder, Manifest
from .pattern import Pattern
from .control_flow import (
    IfElseBuilder, 
    WhileThenElseBuilder,
    build_if_else, 
    build_while_then_else,
    create_break_instruction,
    create_continue_instruction
)

__version__ = "0.1.0"
__author__ = "IonVM Contributors"
__license__ = "MIT"

__all__ = [
    "Value",
    "Instruction", 
    "Function",
    "BytecodeWriter",
    "IonPackBuilder",
    "Manifest",
    "Pattern",
    "IfElseBuilder",
    "WhileThenElseBuilder",
    "build_if_else",
    "build_while_then_else",
    "create_break_instruction",
    "create_continue_instruction",
]
