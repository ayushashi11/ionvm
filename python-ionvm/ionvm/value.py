"""
IonVM Value types and creation utilities.

This module provides the Value class for creating IonVM-compatible values
that can be serialized into bytecode and used in IonVM programs.

The IonVM supports various value types from primitives (numbers, booleans)
to complex structures (objects, arrays, tuples). This module provides
a Python API for creating these values programmatically.
"""
from typing import Dict, List, Any, Optional


class Value:
    """
    Represents an IonVM value that can be serialized to bytecode.
    
    IonVM values are typed data that can be stored in registers, passed as
    arguments, and manipulated by VM instructions. This class provides
    factory methods for creating different types of values.
    
    Attributes:
        value_type: String identifier for the value type
        data: The actual data content of the value
        
    Example:
        >>> # Create basic values
        >>> num = Value.number(42.5)
        >>> text = Value.string("hello world") 
        >>> flag = Value.boolean(True)
        >>> 
        >>> # Create complex values
        >>> items = Value.array([Value.number(1), Value.number(2)])
        >>> obj = Value.object({"x": Value.number(10), "y": Value.number(20)})
    """
    
    def __init__(self, value_type: str, data: Any):
        self.value_type = value_type
        self.data = data
    
    @classmethod
    def number(cls, n: float) -> 'Value':
        """Create a number value."""
        return cls("number", float(n))
    
    @classmethod
    def boolean(cls, b: bool) -> 'Value':
        """Create a boolean value."""
        return cls("boolean", bool(b))
    
    @classmethod
    def atom(cls, s: str) -> 'Value':
        """Create an atom (string) value."""
        return cls("atom", str(s))
    
    @classmethod
    def string(cls, s: str) -> 'Value':
        """Create a string value."""
        return cls("string", str(s))
    
    @classmethod
    def complex(cls, cp: complex) -> 'Value':
        """Create a complex number value."""
        return cls("complex", cp)
    
    @classmethod
    def unit(cls) -> 'Value':
        """Create a unit value."""
        return cls("unit", None)
    
    @classmethod
    def undefined(cls) -> 'Value':
        """Create an undefined value."""
        return cls("undefined", None)
    
    @classmethod
    def array(cls, items: List['Value']) -> 'Value':
        """Create an array value."""
        return cls("array", list(items))
    
    @classmethod
    def object(cls, properties: Dict[str, 'Value'], 
               writable: Optional[Dict[str, bool]] = None,
               enumerable: Optional[Dict[str, bool]] = None,
               configurable: Optional[Dict[str, bool]] = None) -> 'Value':
        """Create an object value with property descriptors."""
        if writable is None:
            writable = {k: True for k in properties}
        if enumerable is None:
            enumerable = {k: True for k in properties}
        if configurable is None:
            configurable = {k: True for k in properties}
            
        obj_data = {
            "properties": properties,
            "writable": writable,
            "enumerable": enumerable,
            "configurable": configurable
        }
        return cls("object", obj_data)
    
    @classmethod
    def function_ref(cls, name: str) -> 'Value':
        """Create a function reference value."""
        return cls("atom", f"__function_ref:{name}")
    
    @classmethod
    def tuple(cls, items: List['Value']) -> 'Value':
        """Create a tuple value."""
        return cls("tuple", list(items))
    
    def serialize(self, writer) -> None:
        """Serialize this value to binary format."""
        if self.value_type == "number":
            writer.write_u8(0x01)  # Number tag
            writer.write_f64(self.data)
        elif self.value_type == "boolean":
            writer.write_u8(0x02)  # Boolean tag
            writer.write_u8(1 if self.data else 0)
        elif self.value_type == "atom":
            writer.write_u8(0x03)  # Atom tag
            writer.write_string(self.data)
        elif self.value_type == "string":
            writer.write_u8(0x09)
            writer.write_string(self.data)
        elif self.value_type == "complex":
            writer.write_u8(0x0A)
            writer.write_f64(self.data.real)
            writer.write_f64(self.data.imag)
        elif self.value_type == "unit":
            writer.write_u8(0x04)  # Unit tag
        elif self.value_type == "undefined":
            writer.write_u8(0x05)  # Undefined tag
        elif self.value_type == "array":
            writer.write_u8(0x06)  # Array tag
            writer.write_u32(len(self.data))
            for item in self.data:
                item.serialize(writer)
        elif self.value_type == "object":
            writer.write_u8(0x07)  # Object tag
            props = self.data["properties"]
            writer.write_u32(len(props))
            for key, value in props.items():
                writer.write_string(key)
                value.serialize(writer)
                writer.write_u8(1 if self.data["writable"].get(key, True) else 0)
                writer.write_u8(1 if self.data["enumerable"].get(key, True) else 0)
                writer.write_u8(1 if self.data["configurable"].get(key, True) else 0)
        elif self.value_type == "function":
            writer.write_u8(0x08)  # Function tag
            writer.write_string(self.data)
        elif self.value_type == "tuple":
            writer.write_u8(0x0B)  # Tuple tag
            writer.write_u32(len(self.data))
            for item in self.data:
                item.serialize(writer)
        else:
            raise ValueError(f"Unsupported value type: {self.value_type}")
    
    def __repr__(self) -> str:
        return f"Value({self.value_type}, {self.data})"
    
    def __eq__(self, other) -> bool:
        if not isinstance(other, Value):
            return False
        return self.value_type == other.value_type and self.data == other.data
