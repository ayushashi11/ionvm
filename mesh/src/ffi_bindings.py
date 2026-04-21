"""
ffi_bindings.py
===============
FFI (Foreign Function Interface) bindings management for the mesh compiler.

This module handles:
- Loading FFI function and object definitions from a bindings file
- Type information for FFI functions and objects
- Native data wrapper support (Java-like objects with native backing data)
- Compiler-side resolution of native references

Bindings File Format (JSON):
{
  "version": "1.0",
  "functions": {
    "debug": {
      "arity": 1,
      "params": ["value"],
      "return_type": "Unit",
      "category": "stdlib"
    },
    "println": {
      "arity": 1,
      "params": ["message"],
      "return_type": "Unit",
      "category": "stdlib"
    }
  },
  "objects": {
    "NativeFile": {
      "methods": {
        "open": {
          "arity": 1,
          "params": ["path"],
          "return_type": "NativeFile",
          "static": true
        },
        "read": {
          "arity": 0,
          "params": [],
          "return_type": "String"
        },
        "close": {
          "arity": 0,
          "params": [],
          "return_type": "Unit"
        }
      },
      "native_backing": true,
      "category": "stdlib"
    }
  }
}
"""

import json
import os
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Set


@dataclass
class FfiFunctionBinding:
    """Represents an FFI function binding."""
    name: str
    arity: int
    params: List[str]
    return_type: str
    category: str = "stdlib"  # stdlib, user, system
    description: str = ""
    vm_name: Optional[str] = None  # Optional name to use in the VM (if different from 'name')
    
    def full_name(self) -> str:
        """Get the full FFI reference name."""
        return f"__{self.category}:{self.vm_name or self.name}"


@dataclass
class FfiMethodBinding:
    """Represents an FFI method binding."""
    name: str
    arity: int
    params: List[str]
    return_type: str
    static: bool = False
    description: str = ""
    
    def full_name(self, class_name: str) -> str:
        """Get the full FFI method reference name."""
        return f"__method:{class_name}:{self.name}"


@dataclass
class FfiObjectBinding:
    """Represents an FFI object type binding."""
    name: str
    methods: Dict[str, FfiMethodBinding] = field(default_factory=dict)
    native_backing: bool = False  # If true, object has native data backing
    category: str = "stdlib"
    description: str = ""
    
    def full_name(self) -> str:
        """Get the full FFI type reference name."""
        return f"__type:{self.category}:{self.name}"


class FfiBindings:
    """Manages FFI function and object bindings."""
    
    def __init__(self):
        self.functions: Dict[str, FfiFunctionBinding] = {}
        self.objects: Dict[str, FfiObjectBinding] = {}
        self._loaded_from_file = False
    
    def load_from_file(self, filepath: str) -> bool:
        """
        Load bindings from a JSON file.
        Returns True if successful, False otherwise.
        """
        if not os.path.exists(filepath):
            print(f"Warning: FFI bindings file not found: {filepath}")
            return False
        
        try:
            with open(filepath, 'r') as f:
                data = json.load(f)
            return self.load_from_dict(data)
        except Exception as e:
            print(f"Error loading FFI bindings from {filepath}: {e}")
            return False
    
    def load_from_dict(self, data: Dict[str, Any]) -> bool:
        """Load bindings from a dictionary (parsed JSON)."""
        try:
            # Load functions
            functions_data = data.get("functions", {})
            for func_name, func_info in functions_data.items():
                self.register_function(
                    name=func_name,
                    arity=func_info.get("arity", 0),
                    params=func_info.get("params", []),
                    return_type=func_info.get("return_type", "Unit"),
                    category=func_info.get("category", "stdlib"),
                    description=func_info.get("description", ""),
                    vm_name=func_info.get("vm_name", None)
                )
            
            # Load objects and their methods
            objects_data = data.get("objects", {})
            for obj_name, obj_info in objects_data.items():
                self.register_object(
                    name=obj_name,
                    native_backing=obj_info.get("native_backing", False),
                    category=obj_info.get("category", "stdlib"),
                    description=obj_info.get("description", "")
                )
                
                # Register methods for this object
                methods_data = obj_info.get("methods", {})
                for method_name, method_info in methods_data.items():
                    self.register_method(
                        class_name=obj_name,
                        method_name=method_name,
                        arity=method_info.get("arity", 0),
                        params=method_info.get("params", []),
                        return_type=method_info.get("return_type", "Unit"),
                        static=method_info.get("static", False),
                        description=method_info.get("description", "")
                    )
            
            self._loaded_from_file = True
            return True
        except Exception as e:
            print(f"Error parsing FFI bindings: {e}")
            return False
    
    def register_function(
        self,
        name: str,
        arity: int,
        params: List[str],
        return_type: str,
        category: str = "stdlib",
        description: str = "",
        vm_name: Optional[str] = None
    ) -> None:
        """Register an FFI function."""
        self.functions[name] = FfiFunctionBinding(
            name=vm_name or name,
            arity=arity,
            params=params,
            return_type=return_type,
            category=category,
            description=description
        )
    
    def register_object(
        self,
        name: str,
        native_backing: bool = False,
        category: str = "stdlib",
        description: str = ""
    ) -> None:
        """Register an FFI object type."""
        self.objects[name] = FfiObjectBinding(
            name=name,
            native_backing=native_backing,
            category=category,
            description=description
        )
    
    def register_method(
        self,
        class_name: str,
        method_name: str,
        arity: int,
        params: List[str],
        return_type: str,
        static: bool = False,
        description: str = ""
    ) -> None:
        """Register a method for an FFI object."""
        if class_name not in self.objects:
            self.register_object(class_name)
        
        self.objects[class_name].methods[method_name] = FfiMethodBinding(
            name=method_name,
            arity=arity,
            params=params,
            return_type=return_type,
            static=static,
            description=description
        )
    
    def get_function(self, name: str) -> Optional[FfiFunctionBinding]:
        """Get a registered FFI function."""
        return self.functions.get(name)
    
    def get_object(self, name: str) -> Optional[FfiObjectBinding]:
        """Get a registered FFI object type."""
        return self.objects.get(name)
    
    def get_method(self, class_name: str, method_name: str) -> Optional[FfiMethodBinding]:
        """Get a method of an FFI object."""
        obj = self.get_object(class_name)
        if obj:
            return obj.methods.get(method_name)
        return None
    
    def is_ffi_function(self, name: str) -> bool:
        """Check if a name is a registered FFI function."""
        return name in self.functions
    
    def is_ffi_object(self, name: str) -> bool:
        """Check if a name is a registered FFI object type."""
        return name in self.objects
    
    def is_ffi_method(self, class_name: str, method_name: str) -> bool:
        """Check if a class has a given method."""
        obj = self.get_object(class_name)
        if obj:
            return method_name in obj.methods
        return False
    
    def is_loaded(self) -> bool:
        """Check if bindings have been loaded from a file."""
        return self._loaded_from_file
    
    
    def to_dict(self) -> Dict[str, Any]:
        """Export bindings to dictionary format."""
        functions_dict = {}
        for name, binding in self.functions.items():
            functions_dict[name] = {
                "arity": binding.arity,
                "params": binding.params,
                "return_type": binding.return_type,
                "category": binding.category,
            }
            if binding.description:
                functions_dict[name]["description"] = binding.description
        
        objects_dict = {}
        for name, obj_binding in self.objects.items():
            methods_dict = {}
            for method_name, method in obj_binding.methods.items():
                methods_dict[method_name] = {
                    "arity": method.arity,
                    "params": method.params,
                    "return_type": method.return_type,
                    "static": method.static,
                }
                if method.description:
                    methods_dict[method_name]["description"] = method.description
            
            objects_dict[name] = {
                "methods": methods_dict,
                "native_backing": obj_binding.native_backing,
                "category": obj_binding.category,
            }
            if obj_binding.description:
                objects_dict[name]["description"] = obj_binding.description
        
        return {
            "version": "1.0",
            "functions": functions_dict,
            "objects": objects_dict,
        }


# Global FFI bindings instance
_global_ffi_bindings: Optional[FfiBindings] = None


def get_global_ffi_bindings() -> FfiBindings:
    """Get or create the global FFI bindings registry."""
    global _global_ffi_bindings
    if _global_ffi_bindings is None:
        _global_ffi_bindings = FfiBindings()
    return _global_ffi_bindings


def load_ffi_bindings(filepath: str) -> bool:
    """Load FFI bindings from a file into the global registry."""
    bindings = get_global_ffi_bindings()
    return bindings.load_from_file(filepath)


def reset_ffi_bindings() -> None:
    """Reset the global FFI bindings registry."""
    global _global_ffi_bindings
    _global_ffi_bindings = None
