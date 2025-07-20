"""
Pattern types for IonVM pattern matching (mirrors Rust enum in vm.rs).
"""
from typing import List, Any
from .value import Value

class Pattern:
    def __init__(self, kind: str, data: Any = None):
        self.kind = kind
        self.data = data

    @classmethod
    def value(cls, value: Value) -> 'Pattern':
        return cls("value", value)

    @classmethod
    def wildcard(cls) -> 'Pattern':
        return cls("wildcard")

    @classmethod
    def tuple(cls, patterns: List['Pattern']) -> 'Pattern':
        return cls("tuple", patterns)

    @classmethod
    def array(cls, patterns: List['Pattern']) -> 'Pattern':
        return cls("array", patterns)

    @classmethod
    def tagged_enum(cls, tag: str, pattern: 'Pattern') -> 'Pattern':
        return cls("tagged_enum", (tag, pattern))

    def serialize(self, writer) -> None:
        print(f"Serializing pattern of kind: {self.kind}")
        if self.kind == "value":
            writer.write_u8(1)
            self.data.serialize(writer)
        elif self.kind == "wildcard":
            writer.write_u8(2)
        elif self.kind == "tuple":
            writer.write_u8(3)
            writer.write_u32(len(self.data))
            for pat in self.data:
                pat.serialize(writer)
        elif self.kind == "array":
            writer.write_u8(4)
            writer.write_u32(len(self.data))
            for pat in self.data:
                pat.serialize(writer)
        elif self.kind == "tagged_enum":
            writer.write_u8(5)
            tag, pat = self.data
            writer.write_string(tag)
            pat.serialize(writer)
        else:
            raise ValueError(f"Unknown pattern kind: {self.kind}")

    def __repr__(self):
        return f"Pattern({self.kind}, {self.data})"
