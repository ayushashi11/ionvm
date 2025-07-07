"""
IonPack format creation and management.
"""
import zipfile
import io
from typing import BinaryIO, Dict, List, Optional
from .function import Function
from .bytecode import BytecodeWriter


class Manifest:
    """
    Represents an IonPack manifest (META-INF/MANIFEST.ion).
    """
    
    def __init__(self, name: str, version: str):
        self.name = name
        self.version = version
        self.main_class: Optional[str] = None
        self.entry_point: Optional[str] = None
        self.description: Optional[str] = None
        self.author: Optional[str] = None
        self.dependencies: List[str] = []
        self.ffi_libraries: List[str] = []
        self.exports: List[str] = []
        self.ionpack_version = "1.0"
    
    def to_string(self) -> str:
        """Serialize manifest to MANIFEST.ion format."""
        lines = [
            f"IonPack-Version: {self.ionpack_version}",
            f"Name: {self.name}",
            f"Version: {self.version}",
        ]
        
        if self.main_class:
            lines.append(f"Main-Class: {self.main_class}")
        
        if self.entry_point:
            lines.append(f"Entry-Point: {self.entry_point}")
        
        if self.description:
            lines.append(f"Description: {self.description}")
        
        if self.author:
            lines.append(f"Author: {self.author}")
        
        if self.dependencies:
            lines.append(f"Dependencies: {', '.join(self.dependencies)}")
        
        if self.ffi_libraries:
            lines.append(f"FFI-Libraries: {', '.join(self.ffi_libraries)}")
        
        if self.exports:
            lines.append(f"Exports: {', '.join(self.exports)}")
        
        return '\n'.join(lines) + '\n'


class IonPackBuilder:
    """
    Builder for creating IonPack files.
    """
    
    def __init__(self, name: str, version: str):
        self.manifest = Manifest(name, version)
        self.classes: Dict[str, bytes] = {}
        self.libraries: Dict[str, bytes] = {}
        self.resources: Dict[str, bytes] = {}
        self.sources: Dict[str, str] = {}
    
    def main_class(self, main_class: str) -> 'IonPackBuilder':
        """Set the main class."""
        self.manifest.main_class = main_class
        return self
    
    def entry_point(self, entry_point: str) -> 'IonPackBuilder':
        """Set the entry point function."""
        self.manifest.entry_point = entry_point
        return self
    
    def description(self, description: str) -> 'IonPackBuilder':
        """Set the package description."""
        self.manifest.description = description
        return self
    
    def author(self, author: str) -> 'IonPackBuilder':
        """Set the package author."""
        self.manifest.author = author
        return self
    
    def dependency(self, dep: str) -> 'IonPackBuilder':
        """Add a dependency."""
        self.manifest.dependencies.append(dep)
        return self
    
    def export(self, export: str) -> 'IonPackBuilder':
        """Add an export."""
        self.manifest.exports.append(export)
        return self
    
    def add_class(self, name: str, function: Function) -> None:
        """Add a single function as a class."""
        buffer = io.BytesIO()
        writer = BytecodeWriter(buffer)
        writer.write_function(function)
        self.classes[name] = buffer.getvalue()
    
    def add_multi_function_class(self, name: str, functions: List[Function]) -> None:
        """Add multiple functions as a single class."""
        buffer = io.BytesIO()
        writer = BytecodeWriter(buffer)
        writer.write_functions(functions)
        self.classes[name] = buffer.getvalue()
    
    def add_library(self, name: str, data: bytes) -> None:
        """Add an FFI library."""
        self.libraries[name] = data
        self.manifest.ffi_libraries.append(name)
    
    def add_resource(self, path: str, data: bytes) -> None:
        """Add a resource file."""
        self.resources[path] = data
    
    def add_source(self, path: str, source: str) -> None:
        """Add source code."""
        self.sources[path] = source
    
    def build(self, stream: BinaryIO) -> None:
        """Build the IonPack file."""
        with zipfile.ZipFile(stream, 'w', zipfile.ZIP_DEFLATED) as zf:
            # Write manifest
            zf.writestr("META-INF/MANIFEST.ion", self.manifest.to_string())
            
            # Write classes
            for name, bytecode in self.classes.items():
                zf.writestr(f"classes/{name}.ionc", bytecode)
            
            # Write libraries
            for name, data in self.libraries.items():
                zf.writestr(f"lib/{name}", data)
            
            # Write resources
            for path, data in self.resources.items():
                zf.writestr(f"resources/{path}", data)
            
            # Write sources
            for path, source in self.sources.items():
                zf.writestr(f"src/{path}", source.encode('utf-8'))
