#!/usr/bin/env python3
"""
Actor model example using the IonVM Python library.
"""
import sys
import os

# Add the library to the Python path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from ionvm import Function, Instruction, Value, IonPackBuilder

def main():
    # Create the coordinator function
    coordinator_function = Function(
        name="coordinator",
        arity=0,
        extra_regs=22,
        instructions=[
            # Debug: Start
            Instruction.load_const(0, Value.atom("__stdlib:debug")),
            Instruction.load_const(1, Value.atom("PYTHON: Starting coordinator")),
            Instruction.call(2, 0, [1]),
            
            # Load the worker function reference for spawning
            Instruction.load_const(3, Value.atom("__function_ref:Actors:worker")),
            
            # Get self reference to pass to worker
            Instruction.load_const(4, Value.atom("__vm:self")),
            
            # Spawn worker process, passing self as argument
            Instruction.spawn(8, 3, [4]),  # r8 = spawn(worker, [self])
            
            # Send a computation task to worker
            Instruction.load_const(12, Value.number(11.23)),
            Instruction.send(8, 12),  # send(worker, 15)
            Instruction.load_const(15, Value.atom("__stdlib:debug")),  # r15 = 15
            Instruction.load_const(16, Value.string("main")),
            Instruction.call(17, 15, [16]),  # r17 = debug("worker")
            # Wait for result from worker
            Instruction.receive(16),  # r16 = receive() (should get 225 = 15*15)
            
            # Return the computed result
            Instruction.return_reg(16),
        ]
    )
    
    # Create the worker function that squares numbers
    worker_function = Function(
        name="worker",
        arity=1,
        extra_regs=23,
        instructions=[
            # Receive task from coordinator
            Instruction.receive(1),  # r1 = receive number to square
            
            # Square the number: result = number * number
            Instruction.mul(2, 1, 1),  # r2 = r1 * r1
            Instruction.load_const(15, Value.atom("__stdlib:debug")),  # r15 = 15
            Instruction.load_const(16, Value.string("worker")),
            Instruction.call(17, 15, [16]), 
            # Send result back to coordinator (r0 contains coordinator process)
            Instruction.send(0, 2),  # send(coordinator, result)
            
            # Worker completes
            Instruction.return_reg(2),
        ]
    )
    
    # Create an IonPack using multi-function format
    builder = IonPackBuilder("python-actors", "1.0.0")
    builder.main_class("Actors")
    builder.entry_point("coordinator")
    builder.description("Actor model demonstration created with Python library")
    builder.author("Python IonVM Library")
    
    # Add both functions to a single class
    builder.add_multi_function_class("Actors", [coordinator_function, worker_function])
    
    # Add source code for reference
    builder.add_source("actors.ion", """
// Actor Model Demonstration created with Python library

function coordinator() {
    // Spawn worker and pass self-reference
    const worker = spawn("worker", [self]);
    
    // Send a number to be squared
    send(worker, 15);
    
    // Wait for the squared result
    const result = receive(); // Should get 225 (15 * 15)
    
    return result;
}

function worker(coordinator) {
    // Wait for task from coordinator
    const number = receive();
    
    // Compute the square
    const result = number * number;
    
    // Send result back
    send(coordinator, result);
    
    return result;
}
""")
    
    # Build the package
    with open("python_actors.ionpack", "wb") as f:
        builder.build(f)
    
    print("Created python_actors.ionpack successfully!")
    print("You can run it with: cargo run --bin ionvm run python_actors.ionpack")
    print("Expected result: 225.0 (15 squared)")

if __name__ == "__main__":
    main()
