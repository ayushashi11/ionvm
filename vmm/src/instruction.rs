use crate::value::{ObjectInitArg, Value};

// Full instruction set for the IonVM register-based bytecode.
//
// Instructions are grouped by category:
//   Memory:       LoadConst, Move
//   Arithmetic:   Add, Sub, Mul, Div
//   Comparison:   Equal, NotEqual, LessThan, LessEqual, GreaterThan, GreaterEqual
//   Logical:      And, Or, Not
//   Object:       ObjectInit, GetProp, SetProp
//   Control flow: Jump, JumpIfTrue, JumpIfFalse, Call, Return
//   Actor:        Spawn, Send, Receive, ReceiveWithTimeout, Link, Select, SelectWithKill
//   Misc:         Match, Yield, Nop
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    // Load a constant value into a register.
    // Special __vm: atoms are resolved at runtime (self, pid, processes, scheduler_passes).
    LoadConst(usize, Value),

    // Copy a value from one register to another.
    Move(usize, usize),

    // Arithmetic: dst = a OP b
    // Numbers, complex numbers, and strings (Add concatenates, Mul repeats strings).
    Add(usize, usize, usize),
    Sub(usize, usize, usize),
    Mul(usize, usize, usize),
    Div(usize, usize, usize),

    // Comparisons: dst = (a OP b)
    Equal(usize, usize, usize),
    NotEqual(usize, usize, usize),
    LessThan(usize, usize, usize),
    LessEqual(usize, usize, usize),
    GreaterThan(usize, usize, usize),
    GreaterEqual(usize, usize, usize),

    // Logical: dst = (a OP b) using truthiness
    And(usize, usize, usize),
    Or(usize, usize, usize),
    Not(usize, usize),

    // Create an object literal with named properties.
    // Each property can come from a register or an inline value,
    // with an optional explicit PropertyAccess level.
    ObjectInit(usize, Vec<(String, ObjectInitArg)>),

    // Read a property from an object into a register.
    // Traverses the prototype chain. Binds `this` if the result is a function.
    GetProp(usize, usize, usize),

    // Write a value from a register into an object property.
    SetProp(usize, usize, usize),

    // Unconditional jump by a signed offset from the current IP.
    Jump(isize),

    // Create an array from a list of registers.
    ArrayInit(usize, Vec<usize>),

    // Conditional jumps: jump if the condition register is truthy/falsy.
    JumpIfTrue(usize, isize),
    JumpIfFalse(usize, isize),

    // Call a function (bytecode or FFI). Pushes a new frame for bytecode functions.
    Call(usize, usize, Vec<usize>),

    // Create a closure value from a function template register plus captured registers.
    // scope_id identifies the lexical closure scope so sibling closures can share env.
    MakeClosure(usize, usize, String, Vec<(String, usize)>),

    // Return from the current function with the value in a register.
    Return(usize),

    // Spawn a new process running the given function with the given arguments.
    Spawn(usize, usize, Vec<usize>),

    // Send a message to another process's mailbox.
    Send(usize, usize),

    // Block until a message arrives, then store it in a register.
    Receive(usize),

    // Like Receive but with a timeout (milliseconds in a register).
    // Sets result_reg to true if a message arrived, false if timeout expired.
    ReceiveWithTimeout(usize, usize, usize),

    // Block until the given process ID exits, then store its return value.
    Link(usize, usize),

    // Match a value against a list of patterns. Jump to the first match.
    Match(usize, Vec<(Pattern, isize)>),

    // Yield control (reserved for generator/coroutine semantics, not yet implemented).
    Yield,

    // No operation.
    Nop,

    // Block until the first of the listed process IDs exits, store its return value.
    Select(usize, Vec<usize>),

    // Like Select, but kills all other listed processes once one exits.
    SelectWithKill(usize, Vec<usize>),
}

impl Instruction {
    // Reduction cost for BEAM-style scheduling.
    // Most instructions cost 1 reduction. Expensive operations cost more.
    // Default budget per process per turn is 2000 reductions (same as BEAM).
    pub fn reduction_cost(&self) -> u32 {
        match self {
            // Message passing and process creation are the most expensive
            Instruction::Spawn(..) | Instruction::Send(..) => 4,
            // Blocking / synchronisation operations
            Instruction::Call(..)
            | Instruction::Receive(..)
            | Instruction::ReceiveWithTimeout(..)
            | Instruction::Link(..)
            | Instruction::Select(..)
            | Instruction::SelectWithKill(..) => 2,
            // Everything else
            _ => 1,
        }
    }
}

// Patterns used by the Match instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    // Match a specific value exactly.
    Value(Value),
    // Match anything.
    Wildcard,
    // Match a tuple and recursively match its elements.
    Tuple(Vec<Pattern>),
    // Match an array and recursively match its elements.
    Array(Vec<Pattern>),
    // Match a tagged enum by tag name and recursively match its payload.
    TaggedEnum(String, Box<Pattern>),
}
