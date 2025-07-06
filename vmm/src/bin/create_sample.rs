use std::fs::File;
use std::env;
use vmm::ionpack::IonPackBuilder;
use vmm::value::{Function, Value, Primitive, Object, PropertyDescriptor};
use vmm::vm::Instruction;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    
    match args.get(1).map(|s| s.as_str()) {
        Some("hello") => create_hello_sample()?,
        Some("complex") => create_complex_sample()?,
        Some("actors") => create_actors_sample()?,
        Some("unified") => create_unified_actors_sample()?,
        _ => {
            println!("Usage: create_sample [hello|complex|actors|unified]");
            println!("  hello   - Simple function returning 42");
            println!("  complex - Object manipulation example");
            println!("  actors  - Cross-process communication example");
            println!("  unified - Complete actor model in single file");
            create_hello_sample()?;
            create_complex_sample()?;
            create_actors_sample()?;
            create_unified_actors_sample()?;
        }
    }
    
    Ok(())
}

fn create_unified_actors_sample() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating unified-actors.ionpack - complete actor model in one file...");

    // Create the coordinator function
    let coordinator_function = Function::new_bytecode(
        Some("coordinator".to_string()),
        0, // No arguments
        22, // Need r0-r21 for all the temp values and function calls
        vec![
            // Debug: Start
            Instruction::LoadConst(0, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("UNIFIED: Starting coordinator".to_string()))),
            Instruction::Call(2, 0, vec![1]),
            
            // Load the worker function reference for spawning
            Instruction::LoadConst(3, Value::Primitive(Primitive::Atom("__function_ref:worker".to_string()))),
            
            // Get self reference to pass to worker
            Instruction::LoadConst(4, Value::Primitive(Primitive::Atom("__vm:self".to_string()))),
            
            // Debug: About to spawn
            Instruction::LoadConst(5, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(6, Value::Primitive(Primitive::Atom("UNIFIED: Spawning worker with self reference".to_string()))),
            Instruction::Call(7, 5, vec![6]),
            
            // Spawn worker process, passing self as argument
            Instruction::Spawn(8, 3, vec![4]), // r8 = spawn(worker, [self])
            
            // Debug: Worker spawned
            Instruction::LoadConst(9, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(10, Value::Primitive(Primitive::Atom("UNIFIED: Worker spawned, sending task".to_string()))),
            Instruction::Call(11, 9, vec![10]),
            
            // Send a computation task to worker
            Instruction::LoadConst(12, Value::Primitive(Primitive::Number(15.0))),
            Instruction::Send(8, 12), // send(worker, 15)
            
            // Debug: Task sent, waiting for result
            Instruction::LoadConst(13, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(14, Value::Primitive(Primitive::Atom("UNIFIED: Task sent, waiting for result".to_string()))),
            Instruction::Call(15, 13, vec![14]),
            
            // Wait for result from worker
            Instruction::Receive(16), // r16 = receive() (should get 225 = 15*15)
            
            // Debug: Got result
            Instruction::LoadConst(17, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(18, Value::Primitive(Primitive::Atom("UNIFIED: Received result from worker".to_string()))),
            Instruction::Call(19, 17, vec![18]),
            
            // Debug: Show the actual result
            Instruction::LoadConst(20, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::Call(21, 20, vec![16]),
            
            // Return the computed result
            Instruction::Return(16),
        ]
    );

    // Create the worker function that squares numbers and sends them back
    let worker_function = Function::new_bytecode(
        Some("worker".to_string()),
        1, // Takes coordinator process as argument
        23, // Need r0 (arg) + r1-r23 for calculations and function calls
        vec![
            // Debug: Worker started
            Instruction::LoadConst(8, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(9, Value::Primitive(Primitive::Atom("UNIFIED: Worker started, coordinator in r0".to_string()))),
            Instruction::Call(10, 8, vec![9]),
            
            // Receive task from coordinator
            Instruction::Receive(1), // r1 = receive number to square
            
            // Debug: Received task
            Instruction::LoadConst(11, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(12, Value::Primitive(Primitive::Atom("UNIFIED: Worker received number to square".to_string()))),
            Instruction::Call(13, 11, vec![12]),
            
            // Debug: Show received number
            Instruction::LoadConst(14, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::Call(15, 14, vec![1]),
            
            // Square the number: result = number * number
            Instruction::Mul(2, 1, 1), // r2 = r1 * r1
            
            // Debug: Computed result
            Instruction::LoadConst(16, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(17, Value::Primitive(Primitive::Atom("UNIFIED: Worker computed square".to_string()))),
            Instruction::Call(18, 16, vec![17]),
            
            // Debug: Show computed result
            Instruction::LoadConst(19, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::Call(20, 19, vec![2]),
            
            // Send result back to coordinator (r0 contains coordinator process)
            Instruction::Send(0, 2), // send(coordinator, result)
            
            // Debug: Result sent
            Instruction::LoadConst(21, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(22, Value::Primitive(Primitive::Atom("UNIFIED: Worker sent result back to coordinator".to_string()))),
            Instruction::Call(23, 21, vec![22]),
            
            // Worker completes
            Instruction::Return(2),
        ]
    );

    // Build the IonPack using multi-function format
    let mut builder = IonPackBuilder::new("unified-actors".to_string(), "1.0.0".to_string())
        .main_class("UnifiedActors".to_string())
        .description("Complete actor model demonstration - both functions in single file".to_string())
        .author("IonVM Developer".to_string());

    // Add both functions to a single class using the new multi-function format
    let functions = vec![coordinator_function, worker_function];
    builder.add_multi_function_class("UnifiedActors", &functions)?;
    
    // Add source code showing the unified pattern
    builder.add_source("unified_actors.ion", r#"
// Unified Actor Model Demonstration
// Both coordinator and worker functions in a single class file

function coordinator() {
    // This is the main coordinator that:
    // 1. Spawns a worker process
    // 2. Sends it a task
    // 3. Waits for the result
    // 4. Returns the computed value
    
    // Spawn worker and pass self-reference so it can send back
    const worker = spawn("worker_func", [self]);
    
    // Send a number to be squared
    send(worker, 15);
    
    // Wait for the squared result
    const result = receive(); // Should get 225 (15 * 15)
    
    return result;
}

function worker_func(coordinator) {
    // This worker function:
    // 1. Receives a task (number to square)
    // 2. Computes the result
    // 3. Sends it back to the coordinator
    
    // Wait for task from coordinator
    const number = receive();
    
    // Compute the square
    const result = number * number;
    
    // Send result back
    send(coordinator, result);
    
    return result;
}

// This demonstrates the complete actor pattern:
// 1. Process Spawning: Create new concurrent processes
// 2. Message Passing: Send data between processes  
// 3. Synchronization: Coordinate using receive/send
// 4. Isolation: Each process has its own state
"#.to_string());

    // Create the package file
    let file = File::create("unified-actors.ionpack")?;
    builder.build(file)?;

    println!("Created unified-actors.ionpack successfully!");
    println!("This demonstrates the complete actor model in a single file.");
    println!();
    println!("Test with:");
    println!("  cargo run --bin ionvm run unified-actors.ionpack");
    println!("  cargo run --bin iondis unified-actors.ionpack unified_main");
    println!("  cargo run --bin iondis unified-actors.ionpack worker_func");
    println!();
    println!("Expected result: 225.0 (15 squared)");
    
    Ok(())
}

fn create_hello_sample() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating hello.ionpack - simple function returning 42...");

    // Create a simple function that returns 42
    let main_function = Function::new_bytecode(
        Some("main".to_string()),
        0,
        1, // Need r0 for the return value
        vec![
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
            Instruction::Return(0),
        ]
    );

    // Build the IonPack
    let mut builder = IonPackBuilder::new("hello-world".to_string(), "1.0.0".to_string())
        .main_class("Main".to_string())
        .description("Simple hello world example".to_string())
        .author("IonVM Developer".to_string());

    builder.add_class("Main", &main_function)?;
    
    builder.add_source("Main.ion", r#"
function main() {
    return 42;
}
"#.to_string());

    let file = File::create("hello.ionpack")?;
    builder.build(file)?;

    println!("Created hello.ionpack successfully!");
    println!("Commands to try:");
    println!("  cargo run --bin ionvm run hello.ionpack");
    println!("  cargo run --bin iondis hello.ionpack Main");
    
    Ok(())
}

fn create_complex_sample() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating complex.ionpack - object manipulation example...");

    // Create an object to be loaded as a constant
    let mut object_props = HashMap::new();
    object_props.insert("name".to_string(), PropertyDescriptor {
        value: Value::Primitive(Primitive::Atom("Alice".to_string())),
        writable: true,
        enumerable: true,
        configurable: true,
    });
    object_props.insert("age".to_string(), PropertyDescriptor {
        value: Value::Primitive(Primitive::Number(30.0)),
        writable: true,
        enumerable: true,
        configurable: true,
    });
    object_props.insert("active".to_string(), PropertyDescriptor {
        value: Value::Primitive(Primitive::Boolean(true)),
        writable: true,
        enumerable: true,
        configurable: true,
    });
    
    let object = Object {
        properties: object_props,
        prototype: None,
        magic_methods: HashMap::new(),
        type_name: Some("User".to_string()),
    };
    let object_value = Value::Object(Rc::new(RefCell::new(object)));

    // Create a main function that loads the object constant and returns it
    let main_function = Function::new_bytecode(
        Some("main".to_string()),
        0,
        1, // Need r0 for object return value
        vec![
            Instruction::LoadConst(0, object_value),
            Instruction::Return(0),
        ]
    );

    // Create a helper function that accesses object properties
    let helper_function = Function::new_bytecode(
        Some("get_user_info".to_string()),
        1, // Takes one argument (the object)
        4, // Need r1, r2, r3, r4 for property access
        vec![
            // Load property keys
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("name".to_string()))),
            Instruction::LoadConst(2, Value::Primitive(Primitive::Atom("age".to_string()))),
            // Get properties from the object in r0 (argument)
            Instruction::GetProp(3, 0, 1), // r3 = obj.name
            Instruction::GetProp(4, 0, 2), // r4 = obj.age
            // Return the name (could do more complex processing)
            Instruction::Return(3),
        ]
    );

    // Build the IonPack
    let mut builder = IonPackBuilder::new("complex-example".to_string(), "1.0.0".to_string())
        .main_class("Main".to_string())
        .description("Complex example with object manipulation".to_string())
        .author("IonVM Developer".to_string());

    builder.add_class("Main", &main_function)?;
    builder.add_class("UserHelper", &helper_function)?;
    
    // Add source files for reference
    builder.add_source("Main.ion", r#"
function main() {
    const user = {
        name: "Alice",
        age: 30,
        active: true
    };
    return user;
}
"#.to_string());
    
    builder.add_source("UserHelper.ion", r#"
function get_user_info(user) {
    return user.name;
}
"#.to_string());

    // Create the package file
    let file = File::create("complex.ionpack")?;
    builder.build(file)?;

    println!("Created complex.ionpack successfully!");
    println!("Commands to try:");
    println!("  cargo run --bin ionvm run complex.ionpack");
    println!("  cargo run --bin iondis complex.ionpack Main");
    
    Ok(())
}

fn create_actors_sample() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating actors.ionpack - cross-process communication example...");

    // Create a worker function that receives messages, processes them, and sends results back
    let worker_function = Function::new_bytecode(
        Some("worker".to_string()),
        1, // Takes coordinator process as argument
        23, // Need r1-r23 for all the debug calls and calculations
        vec![
            // Debug: Worker started
            Instruction::LoadConst(8, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(9, Value::Primitive(Primitive::Atom("WORKER: Started, waiting for message".to_string()))),
            Instruction::Call(10, 8, vec![9]),
            
            // Worker: receive message, process it, send result back to coordinator
            Instruction::Receive(1),        // r1 = receive message from coordinator
            
            // Debug: Received message
            Instruction::LoadConst(11, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(12, Value::Primitive(Primitive::Atom("WORKER: Received message".to_string()))),
            Instruction::Call(13, 11, vec![12]),
            
            // Debug: Print the received value
            Instruction::LoadConst(14, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::Call(15, 14, vec![1]),
            
            Instruction::LoadConst(2, Value::Primitive(Primitive::Number(2.0))),
            Instruction::Mul(3, 1, 2),      // r3 = message * 2 (process the message)
            
            // Debug: Processed result
            Instruction::LoadConst(16, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(17, Value::Primitive(Primitive::Atom("WORKER: Processed result".to_string()))),
            Instruction::Call(18, 16, vec![17]),
            
            // Debug: Print the processed value
            Instruction::LoadConst(19, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::Call(20, 19, vec![3]),
            
            // Send the result back to coordinator (in r0)
            Instruction::Send(0, 3),        // send(coordinator_process, result)
            
            // Debug: Sent result back
            Instruction::LoadConst(21, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(22, Value::Primitive(Primitive::Atom("WORKER: Sent result back to coordinator".to_string()))),
            Instruction::Call(23, 21, vec![22]),
            
            // Worker can return or continue processing
            Instruction::Return(3),         // return the processed value
        ]
    );

    // Create a simple echo worker for testing
    let echo_worker = Function::new_bytecode(
        Some("echo_worker".to_string()),
        0,
        1, // Need r0 for receive and return
        vec![
            Instruction::Receive(0),        // r0 = receive message
            Instruction::Return(0),         // echo it back
        ]
    );

    // Create the main function that actually uses SPAWN, SEND, and RECEIVE
    let main_function = Function::new_bytecode(
        Some("main".to_string()),
        0,
        27, // Need r0-r26 for all the debug calls and process management
        vec![
            // Debug: Print start message
            Instruction::LoadConst(0, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("MAIN: Starting execution".to_string()))),
            Instruction::Call(2, 0, vec![1]),
            
            // Step 1: Load worker function reference using special marker for resolution
            Instruction::LoadConst(3, Value::Primitive(Primitive::Atom("__function_ref:Worker".to_string()))),
            
            // Debug: Print worker function loaded
            Instruction::LoadConst(4, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(5, Value::Primitive(Primitive::Atom("MAIN: Worker function reference loaded".to_string()))),
            Instruction::Call(6, 4, vec![5]),
            
            // Step 2: Get self process reference (coordinator)
            Instruction::LoadConst(7, Value::Primitive(Primitive::Atom("__vm:self".to_string()))),
            
            // Debug: Print before spawn
            Instruction::LoadConst(8, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(9, Value::Primitive(Primitive::Atom("MAIN: About to spawn worker process".to_string()))),
            Instruction::Call(10, 8, vec![9]),
            
            // Step 3: Spawn worker process with coordinator as argument
            Instruction::Spawn(11, 3, vec![7]), // r11 = spawn(worker_function, [self])
            
            // Debug: Print after spawn
            Instruction::LoadConst(12, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(13, Value::Primitive(Primitive::Atom("MAIN: Successfully spawned worker process".to_string()))),
            Instruction::Call(14, 12, vec![13]),
            
            // Step 4: Load message to send to worker  
            Instruction::LoadConst(15, Value::Primitive(Primitive::Number(25.0))),
            
            // Debug: Print before send
            Instruction::LoadConst(16, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(17, Value::Primitive(Primitive::Atom("MAIN: About to send message to worker".to_string()))),
            Instruction::Call(18, 16, vec![17]),
            
            // Step 5: Send message to worker
            Instruction::Send(11, 15), // send(worker_process, 25)
            
            // Debug: Print after send
            Instruction::LoadConst(19, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(20, Value::Primitive(Primitive::Atom("MAIN: Message sent, now waiting for response".to_string()))),
            Instruction::Call(21, 19, vec![20]),
            
            // Step 6: Receive result back from worker
            Instruction::Receive(22), // r22 = receive() (should get 50 back from worker)
            
            // Debug: Print received result
            Instruction::LoadConst(23, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::LoadConst(24, Value::Primitive(Primitive::Atom("MAIN: Received response from worker".to_string()))),
            Instruction::Call(25, 23, vec![24]),
            
            // Debug: Print final result value
            Instruction::LoadConst(26, Value::Primitive(Primitive::Atom("__stdlib:debug".to_string()))),
            Instruction::Call(27, 26, vec![22]), // Debug print the actual result value
            
            // Step 7: Return the received result
            Instruction::Return(22),  // return received result
        ]
    );

    // Create a working cross-process demo using simpler approach
    let cross_process_demo = Function::new_bytecode(
        Some("cross_process_demo".to_string()),
        0,
        1, // Need r0 for return value
        vec![
            // This function shows the actual cross-process pattern that works
            // It assumes the worker function is available in register 0 (from external setup)
            
            // Load message to send to worker
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
            
            // In real usage, worker would be spawned like this:
            // Instruction::LoadClass(1, "Worker"),     // Load worker function 
            // Instruction::Spawn(2, 1, vec![]),       // r2 = spawn(worker)
            // Instruction::Send(2, 0),                // send message to worker
            // Instruction::Receive(3),                // r3 = receive response
            // Instruction::Return(3),                 // return the response
            
            // For demo, just return the message
            Instruction::Return(0),
        ]
    );

    // Create a ping-pong demonstration
    let ping_function = Function::new_bytecode(
        Some("ping".to_string()),
        1, // Takes pong process as argument
        2, // Need r1, r2 for message and response
        vec![
            // Send "ping" message to pong process (in r0)
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("ping".to_string()))),
            Instruction::Send(0, 1),        // send to process in r0
            
            // Wait for pong response
            Instruction::Receive(2),        // r2 = receive response
            
            // Return the response
            Instruction::Return(2),
        ]
    );

    let pong_function = Function::new_bytecode(
        Some("pong".to_string()),
        0,
        1, // Need r0, r1 for receive and return
        vec![
            // Wait for ping message
            Instruction::Receive(0),        // r0 = receive "ping"
            
            // Send back "pong" 
            // Note: In a real scenario, we'd need sender's process ID to send back
            // This simplified version just returns the response
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("pong".to_string()))),
            Instruction::Return(1),
        ]
    );

    // Create a comprehensive coordinator that demonstrates full actor workflow
    let coordinator_function = Function::new_bytecode(
        Some("coordinator".to_string()),
        0,
        4, // Need r0-r4 for the coordination calculations
        vec![
            // Multi-step coordination process
            
            // Step 1: Prepare worker data
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(10.0))), // task 1
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(20.0))), // task 2
            Instruction::LoadConst(2, Value::Primitive(Primitive::Number(30.0))), // task 3
            
            // Step 2: Coordination logic (simplified for demo)
            // In full implementation:
            // - Spawn workers for each task
            // - Send tasks to workers  
            // - Collect results
            // - Aggregate and return
            
            // For demo: sum the tasks to simulate coordination result
            Instruction::Add(3, 0, 1),      // r3 = task1 + task2  
            Instruction::Add(4, 3, 2),      // r4 = sum + task3
            
            // Return coordination result
            Instruction::Return(4),         // return total (60.0)
        ]
    );

    // Build the IonPack
    let mut builder = IonPackBuilder::new("actors-example".to_string(), "1.0.0".to_string())
        .main_class("Main".to_string())
        .description("Actor model with cross-process communication".to_string())
        .author("IonVM Developer".to_string());

    builder.add_class("Main", &main_function)?;
    builder.add_class("Worker", &worker_function)?;
    builder.add_class("EchoWorker", &echo_worker)?;
    builder.add_class("CrossProcessDemo", &cross_process_demo)?;
    builder.add_class("Ping", &ping_function)?;
    builder.add_class("Pong", &pong_function)?;
    builder.add_class("Coordinator", &coordinator_function)?;
    
    // Add comprehensive source files showing the intended actor patterns
    builder.add_source("Main.ion", r#"
// Main function demonstrating bidirectional actor communication
function main() {
    // Note: This shows the bidirectional send-back pattern
    
    // 1. Spawn a worker process, passing self as argument so worker can send back
    const worker = spawn("Worker", [self]);
    
    // 2. Send a task to the worker
    send(worker, 25);
    
    // 3. Receive the result sent back from worker
    const result = receive();  // result should be 50 (25 * 2)
    
    // 4. Return the result
    return result;
}
"#.to_string());
    
    builder.add_source("Worker.ion", r#"
// Worker process that receives and processes messages, then sends results back
function worker(coordinator_process) {
    // Receive a number from coordinator
    const number = receive();
    
    // Process it (double the number)
    const result = number * 2;
    
    // Send result back to coordinator
    send(coordinator_process, result);
    
    // Return the result as well
    return result;
}
"#.to_string());

    builder.add_source("PingPong.ion", r#"
// Ping-pong demonstration between two processes
function ping(pong_process) {
    // Send ping message
    send(pong_process, "ping");
    
    // Wait for pong response
    const response = receive();
    
    return response; // should be "pong"
}

function pong() {
    // Wait for ping
    const message = receive();
    
    // Respond with pong
    return "pong";
}
"#.to_string());

    builder.add_source("Coordinator.ion", r#"
// Coordinator that manages multiple workers
function coordinator() {
    // Spawn multiple workers
    const worker1 = spawn("Worker");
    const worker2 = spawn("Worker"); 
    const worker3 = spawn("Worker");
    
    // Distribute tasks
    send(worker1, 10);
    send(worker2, 20);
    send(worker3, 30);
    
    // Collect results
    const result1 = receive(); // 20
    const result2 = receive(); // 40
    const result3 = receive(); // 60
    
    // Return total
    return result1 + result2 + result3; // 120
}
"#.to_string());

    builder.add_source("CrossProcessDemo.ion", r#"
// Complete cross-process communication example
function cross_process_demo() {
    // This demonstrates the full actor model workflow:
    
    // 1. Process Spawning
    const echo_worker = spawn("EchoWorker");
    const math_worker = spawn("Worker");
    
    // 2. Message Passing
    send(echo_worker, "hello");
    send(math_worker, 25);
    
    // 3. Response Handling
    const echo_response = receive(); // "hello"
    const math_response = receive(); // 50
    
    // 4. Coordination
    return math_response; // 50
}
"#.to_string());

    builder.add_source("README.md", r#"
# Actor Model Cross-Process Communication

This IonPack demonstrates cross-process communication using the actor model in IonVM.

## Key Instructions

### Process Management
- `Spawn(dst, func, args)` - Create new process
- `Send(proc, msg)` - Send message to process  
- `Receive(dst)` - Receive message (blocks until available)
- `Link(proc)` - Link processes for fault tolerance

### Communication Patterns

1. **Request-Response**: Coordinator sends task, waits for result
2. **Pipeline**: Chain processes for data transformation
3. **Worker Pool**: Multiple workers processing tasks concurrently
4. **Ping-Pong**: Bidirectional message exchange

## Example Workflows

### Simple Worker
```
Coordinator -> [task] -> Worker -> [result] -> Coordinator
```

### Multi-Worker
```
Coordinator -> [task1] -> Worker1 -> [result1] -> 
            -> [task2] -> Worker2 -> [result2] -> Coordinator
            -> [task3] -> Worker3 -> [result3] ->
```

### Ping-Pong
```
Ping -> ["ping"] -> Pong -> ["pong"] -> Ping
```

## Classes in this Package

- **Main**: Entry point demonstrating basic spawning
- **Worker**: Doubles received numbers
- **EchoWorker**: Echoes back received messages
- **Ping/Pong**: Bidirectional communication demo
- **Coordinator**: Multi-worker coordination
- **CrossProcessDemo**: Complete workflow example

## Notes

The bytecode demonstrates the instruction patterns for actor model programming.
Full functionality requires VM runtime support for dynamic class loading and 
process management.
"#.to_string());

    // Create the package file
    let file = File::create("actors.ionpack")?;
    builder.build(file)?;

    println!("Created actors.ionpack successfully!");
    println!("This demonstrates real cross-process send/receive patterns.");
    println!();
    println!("Commands to try:");
    println!("  cargo run --bin ionvm run actors.ionpack");
    println!("  cargo run --bin iondis actors.ionpack Main");
    println!("  cargo run --bin iondis actors.ionpack Worker");
    println!("  cargo run --bin iondis actors.ionpack Ping");
    println!("  cargo run --bin iondis actors.ionpack Coordinator");
    println!();
    println!("Key functions with send/receive:");
    println!("  Worker: RECEIVE, MUL, RETURN");
    println!("  Ping: SEND, RECEIVE, RETURN");  
    println!("  Coordinator: Multiple coordination steps");
    
    Ok(())
}
