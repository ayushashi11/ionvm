#!/usr/bin/env python3
"""
Object manipulation example using the IonVM Python library.
"""
import sys
import os

# Add the library to the Python path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from ionvm import Function, Instruction, Value, IonPackBuilder

def main():
    # Create an object value
    user_object = Value.object({
        "name": Value.atom("Alice"),
        "age": Value.number(30),
        "active": Value.boolean(True)
    })
    
    # Create a function that loads and returns the object
    main_function = Function(
        name="main",
        arity=0,
        extra_regs=1,
        instructions=[
            Instruction.load_const(0, user_object),
            Instruction.load_const(1, Value.atom("__stdlib:debug")),
            Instruction.call(1, 1, [0]),  # Call debug function to log the object
            Instruction.return_reg(0)
        ]
    )
    
    # Create a helper function that accesses object properties
    helper_function = Function(
        name="get_user_info",
        arity=1,
        extra_regs=4,
        instructions=[
            # Load property keys
            Instruction.load_const(1, Value.atom("name")),
            Instruction.load_const(2, Value.atom("age")),
            # Get properties from the object in r0 (argument)
            Instruction.get_prop(3, 0, 1),  # r3 = obj.name
            Instruction.get_prop(4, 0, 2),  # r4 = obj.age
            # Return the name
            Instruction.return_reg(3)
        ]
    )
    
    # Create an IonPack
    builder = IonPackBuilder("complex-example", "1.0.0")
    builder.main_class("Main")
    builder.entry_point("main")
    builder.description("Complex example with object manipulation")
    builder.author("Python IonVM Library")
    
    builder.add_class("Main", main_function)
    builder.add_class("UserHelper", helper_function)
    
    # Add source code for reference
    builder.add_source("main.ion", """
function main() {
    const user = {
        name: "Alice",
        age: 30,
        active: true
    };
    return user;
}
""")
    
    builder.add_source("helper.ion", """
function get_user_info(user) {
    return user.name;
}
""")
    
    # Build the package
    with open("complex_python.ionpack", "wb") as f:
        builder.build(f)
    
    print("Created complex_python.ionpack successfully!")
    print("You can run it with: cargo run --bin ionvm run complex_python.ionpack")

if __name__ == "__main__":
    main()
