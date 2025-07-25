use crate::ffi_integration::{FfiCallResult, call_ffi_function};
use crate::value::{Function, ProcessStatus, Value};
use crate::Primitive;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use vm_ffi::FfiRegistry;

// Timeout tracking for receive_with_timeout operations
#[derive(Debug)]
struct TimeoutInfo {
    pid: usize,
    dst_reg: usize,
    result_reg: usize,
    expiry_ms: u64,     // When timeout expires (milliseconds since UNIX_EPOCH)
    frame_index: usize, // Index of the frame for this timeout
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    Continue,        // Continue execution
    Yield,           // Process yielded voluntarily
    BudgetExhausted, // Reduction budget exhausted
    Blocked,         // Process blocked (e.g., waiting for message)
    Exited(Value),   // Process exited with return value
    Error(String),   // Execution error
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// Create an object with a list of (key, value) pairs, where value can be a register or a constant value
    ObjectInit(usize, Vec<(String, crate::value::ObjectInitArg)>), // dst, [(key, value/register)]
    LoadConst(usize, Value),                 // reg, value
    Move(usize, usize),                      // dst, src
    Add(usize, usize, usize),                // dst, a, b
    Sub(usize, usize, usize),                // dst, a, b
    Mul(usize, usize, usize),                // dst, a, b
    Div(usize, usize, usize),                // dst, a, b
    GetProp(usize, usize, usize),            // dst, obj, key
    SetProp(usize, usize, usize),            // obj, key, value
    Call(usize, usize, Vec<usize>),          // dst, func, args
    Return(usize),                           // reg
    Jump(isize),                             // offset
    JumpIfTrue(usize, isize),                // cond_reg, offset
    JumpIfFalse(usize, isize),               // cond_reg, offset
    Spawn(usize, usize, Vec<usize>),         // dst, func, args
    Send(usize, usize),                      // proc, msg
    Receive(usize),                          // dst
    ReceiveWithTimeout(usize, usize, usize), // dst, timeout_reg, result_reg
    Link(usize),                             // proc
    Match(usize, Vec<(Pattern, isize)>),     // src, pattern table (pattern, jump offset)
    Yield,                                   // Explicit yield point
    Nop,
    // Comparison operations
    Equal(usize, usize, usize),        // dst, a, b - equality comparison
    NotEqual(usize, usize, usize),     // dst, a, b - inequality comparison
    LessThan(usize, usize, usize),     // dst, a, b - less than comparison
    LessEqual(usize, usize, usize),    // dst, a, b - less than or equal
    GreaterThan(usize, usize, usize),  // dst, a, b - greater than comparison
    GreaterEqual(usize, usize, usize), // dst, a, b - greater than or equal
    // Logical operations
    And(usize, usize, usize), // dst, a, b - logical AND
    Or(usize, usize, usize),  // dst, a, b - logical OR
    Not(usize, usize),        // dst, src - logical NOT
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Value(Value),
    Wildcard,
    Tuple(Vec<Pattern>),
    Array(Vec<Pattern>),
    TaggedEnum(String, Box<Pattern>),
}

#[derive(Debug)]
pub struct Frame {
    pub registers: Vec<Value>,
    pub stack: Vec<Value>,
    pub ip: usize,
    pub function: Rc<Function>,
    pub return_value: Option<Value>,
    pub caller_return_reg: Option<usize>, // Where to store return value in caller
}

pub struct IonVM {
    pub processes: HashMap<usize, Rc<RefCell<crate::value::Process>>>,
    pub run_queue: VecDeque<usize>,
    pub next_pid: usize,
    pub reduction_limit: u32,
    pub timeslice: u32, // Number of instructions per process before preemption
    pub scheduler_passes: u64,
    pub ffi_registry: FfiRegistry,
    stdlib_functions: Option<HashMap<String, Value>>, // For stdlib function references
    pub debug: bool,                                  // Enable debug output
    pending_timeouts: Vec<TimeoutInfo>,               // Track pending timeout operations
}

impl IonVM {
    pub fn new() -> Self {
        IonVM {
            processes: HashMap::new(),
            run_queue: VecDeque::new(),
            next_pid: 1,
            reduction_limit: 2000, // Standard Erlang reduction count
            timeslice: 3, // Default timeslice for preemptive scheduling
            scheduler_passes: 0,
            ffi_registry: FfiRegistry::with_stdlib(),
            stdlib_functions: None,
            debug: false,
            pending_timeouts: Vec::new(),
        }
    }

    /// Create a new VM with custom FFI registry
    pub fn with_ffi_registry(ffi_registry: FfiRegistry) -> Self {
        IonVM {
            processes: HashMap::new(),
            run_queue: VecDeque::new(),
            next_pid: 1,
            reduction_limit: 2000,
            timeslice: 3,
            scheduler_passes: 0,
            ffi_registry,
            stdlib_functions: None,
            debug: false,
            pending_timeouts: Vec::new(),
        }
    }

    /// Enable or disable debug output
    pub fn set_debug(&mut self, debug: bool) {
        self.debug = debug;
    }

    /// Check for expired timeouts and handle them
    pub fn handle_expired_timeouts(&mut self) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut expired_timeouts = Vec::new();
        let mut remaining_timeouts = Vec::new();

        // Split expired and remaining timeouts
        for timeout in self.pending_timeouts.drain(..) {
            if current_time >= timeout.expiry_ms {
                expired_timeouts.push(timeout);
            } else {
                remaining_timeouts.push(timeout);
            }
        }

        // Keep the remaining timeouts
        self.pending_timeouts = remaining_timeouts;

        // Handle expired timeouts
        for timeout in expired_timeouts {
            if let Some(process_rc) = self.processes.get(&timeout.pid) {
                let mut proc = process_rc.borrow_mut();
                if proc.status == ProcessStatus::WaitingForMessage {
                    // Set timeout result to false in the correct frame
                    if let Some(frame) = proc.frames.get_mut(timeout.frame_index) {
                        frame.registers[timeout.result_reg] =
                            Value::Primitive(crate::value::Primitive::Boolean(false));
                        if self.debug {
                            println!(
                                "\x1b[36m[VM DEBUG]\x1b[0m TIMEOUT: Process {} timed out, set result r{} to false (frame_index {})",
                                timeout.pid, timeout.result_reg, timeout.frame_index
                            );
                        }
                        //increase ip
                        proc.frames.last_mut().unwrap().ip += 1; // Adjust IP to continue execution
                    }
                    // Unblock the process
                    proc.status = ProcessStatus::Runnable;
                    self.run_queue.push_back(timeout.pid);
                    if self.debug {
                        println!(
                            "\x1b[36m[VM DEBUG]\x1b[0m TIMEOUT: Process {} unblocked due to timeout",
                            timeout.pid
                        );
                    }
                }
            }
        }
    }

    /// Create a new VM with debug enabled
    pub fn with_debug() -> Self {
        let mut vm = Self::new();
        vm.debug = true;
        vm
    }

    /// Get an FFI function as a first-class Value
    pub fn get_ffi_function(&self, function_name: &str) -> Option<Value> {
        if let Some((name, arity, _description)) =
            self.ffi_registry.get_function_info(function_name)
        {
            let function =
                Function::new_ffi(Some(name.to_string()), arity, function_name.to_string());
            Some(Value::Function(Rc::new(RefCell::new(function))))
        } else {
            None
        }
    }

    /// Get all available FFI functions as a HashMap
    pub fn get_all_ffi_functions(&self) -> HashMap<String, Value> {
        let mut functions = HashMap::new();
        for function_name in self.ffi_registry.list_functions() {
            if let Some(func_value) = self.get_ffi_function(function_name) {
                functions.insert(function_name.to_string(), func_value);
            }
        }
        functions
    }

    /// Resolve stdlib function references in bytecode
    pub fn resolve_stdlib_functions(&mut self) {
        // Register stdlib functions in FFI registry
        vm_ffi::stdlib::register_all(&mut self.ffi_registry);

        // Convert print functions to VM values for bytecode use
        self.stdlib_functions = Some(HashMap::new());
    }

    /// Get a stdlib function as a VM Value
    pub fn get_stdlib_function(&self, name: &str) -> Option<Value> {
        self.get_ffi_function(name)
    }

    /// Resolve special __vm: values at runtime
    fn resolve_vm_value(&self, val: Value, current_pid: usize, current_frame: &mut Frame) -> Value {
        match &val {
            Value::Primitive(crate::value::Primitive::Atom(atom)) => {
                if atom.starts_with("__vm:") {
                    let vm_command = &atom[5..]; // Remove "__vm:" prefix
                    match vm_command {
                        "self" => {
                            // Get current process reference
                            if let Some(current_proc) = self.processes.get(&current_pid) {
                                Value::Process(current_proc.clone())
                            } else {
                                Value::Primitive(crate::value::Primitive::Undefined)
                            }
                        }
                        "this" => {
                            // Get bound_this
                            if let Some(this) = &current_frame.function.bound_this {
                                return this.clone();
                            } else {
                                return Value::Primitive(crate::value::Primitive::Undefined);
                            }
                        }
                        "pid" => {
                            // Get current process ID as number
                            Value::Primitive(crate::value::Primitive::Number(current_pid as f64))
                        }
                        "processes" => {
                            // Get count of total processes
                            Value::Primitive(crate::value::Primitive::Number(
                                self.processes.len() as f64
                            ))
                        }
                        "scheduler_passes" => {
                            // Get scheduler pass count
                            Value::Primitive(crate::value::Primitive::Number(
                                self.scheduler_passes as f64,
                            ))
                        }
                        _ => {
                            // Unknown __vm: command - return undefined
                            Value::Primitive(crate::value::Primitive::Undefined)
                        }
                    }
                } else if atom.starts_with("__function_ref:") {
                    // Handle function references - these would normally be resolved at load time
                    // For now, return undefined as they need to be handled by the IonPack loader
                    panic!(
                        "Function references should be resolved at load time: {}",
                        atom
                    );
                } else if atom.starts_with("__stdlib:") {
                    // Handle stdlib function references
                    if let Some(func) = self.get_stdlib_function(&atom[9..]) {
                        func
                    } else {
                        Value::Primitive(crate::value::Primitive::Undefined)
                    }
                } else if atom == "self" {
                    // Legacy support for bare 'self' - treat as __vm:self
                    if let Some(current_proc) = self.processes.get(&current_pid) {
                        Value::Process(current_proc.clone())
                    } else {
                        Value::Primitive(crate::value::Primitive::Undefined)
                    }
                } else {
                    // Regular atom - no special handling
                    val
                }
            }
            _ => val, // Not an atom - no special handling
        }
    }

    pub fn spawn_process(&mut self, function: Rc<Function>, args: Vec<Value>) -> usize {
        let pid = self.next_pid;
        self.next_pid += 1;

        if self.debug {
            println!(
                "\x1b[36m[VM DEBUG]\x1b[0m Spawning process {} with function: {:?}",
                pid, function.name
            );
        }

        let process = Rc::new(RefCell::new(crate::value::Process::new(
            pid, function, args,
        )));

        self.processes.insert(pid, process);
        self.run_queue.push_back(pid);

        if self.debug {
            if self.debug {
                println!(
                    "\x1b[36m[VM DEBUG]\x1b[0m Process {} added to run queue. Total processes: {}, Queue length: {}",
                    pid,
                    self.processes.len(),
                    self.run_queue.len()
                );
            }
        }

        pid
    }

    /// Spawn a main process and execute it to completion
    /// Returns the final result value
    pub fn spawn_main_process(&mut self, function: Function) -> Result<Value, String> {
        use crate::value::Primitive;
        use std::rc::Rc;

        // Spawn the main process
        let pid = self.spawn_process(Rc::new(function), vec![]);

        // Run the VM until the main process completes
        loop {
            self.run();

            // Check if the main process is still alive
            if let Some(process) = self.processes.get(&pid) {
                let process_ref = process.borrow();
                if !process_ref.alive {
                    // Process completed, get the return value
                    if let Some(frame) = process_ref.frames.last() {
                        return Ok(frame
                            .return_value
                            .clone()
                            .unwrap_or(Value::Primitive(Primitive::Unit)));
                    } else {
                        return Ok(Value::Primitive(Primitive::Unit));
                    }
                }
                drop(process_ref);
            } else {
                // Process not found, probably completed
                return Ok(Value::Primitive(Primitive::Unit));
            }

            // If no processes are scheduled (deadlock or completion), break
            if self.run_queue.is_empty() {
                break;
            }
        }

        Ok(Value::Primitive(Primitive::Unit))
    }

    /// Main scheduler loop - Erlang-style preemptive scheduling
    pub fn run(&mut self) {
        while self.has_runnable_processes() {
            self.scheduler_passes += 1;

            if self.debug {
                println!(
                    "\x1b[36m[VM DEBUG]\x1b[0m Scheduler pass {}. Run queue: {:?}",
                    self.scheduler_passes, self.run_queue
                );
            }

            // Get next process to run
            if let Some(pid) = self.run_queue.pop_front() {
                if self.debug {
                    println!("\x1b[36m[VM DEBUG]\x1b[0m Executing process {}", pid);
                }
                if let Some(proc_ref) = self.processes.get(&pid).cloned() {
                    let result = self.execute_process_slice(proc_ref.clone());
                    if self.debug {
                        println!(
                            "\x1b[36m[VM DEBUG]\x1b[0m Process {} result: {:?}",
                            pid, result
                        );
                    }
                    self.handle_execution_result(pid, result);
                }
            }

            // After each scheduling pass, check if any blocked processes can be unblocked
            self.check_blocked_processes();
        }

        if self.debug {
            if self.debug {
                println!("\x1b[36m[VM DEBUG]\x1b[0m Scheduler finished. Final process states:");
                for (pid, proc_ref) in &self.processes {
                    let proc = proc_ref.borrow();
                    println!(
                        "\x1b[36m[VM DEBUG]\x1b[0m Process {}: alive={}, status={:?}, mailbox_size={}",
                        pid,
                        proc.alive,
                        proc.status,
                        proc.mailbox.len()
                    );
                }
            }
        }
    }

    /// Check if any processes can be scheduled
    fn has_runnable_processes(&mut self) -> bool {
        self.handle_expired_timeouts();
        self.processes.values().any(|p| {
            let proc = p.borrow();
            proc.alive && proc.status == ProcessStatus::Runnable
        })
    }

    /// Execute a process for up to reduction_limit instructions
    fn execute_process_slice(
        &mut self,
        proc_ref: Rc<RefCell<crate::value::Process>>,
    ) -> ExecutionResult {
        let mut proc = proc_ref.borrow_mut();

        // Reset reduction budget for this scheduling round (use timeslice for preemption)
        proc.reset_reductions(self.timeslice);

        while proc.reductions > 0 && proc.status == ProcessStatus::Runnable {
            // Check if process has frames to execute
            if proc.frames.is_empty() {
                return ExecutionResult::Exited(Value::Primitive(crate::value::Primitive::Unit));
            }

            // Get current instruction
            let instruction = {
                let frame = proc.frames.last().unwrap();
                let ip = frame.ip;

                // Get bytecode from function
                let bytecode = match &frame.function.function_type {
                    crate::value::FunctionType::Bytecode { bytecode } => bytecode,
                    crate::value::FunctionType::Ffi { .. } => {
                        // FFI functions shouldn't have frames - this is an error
                        return ExecutionResult::Error("FFI function in frame stack".to_string());
                    }
                };

                if ip >= bytecode.len() {
                    // End of function - handle return
                    let return_val = frame
                        .return_value
                        .clone()
                        .unwrap_or(Value::Primitive(crate::value::Primitive::Unit));

                    // Pop current frame
                    let finished_frame = proc.frames.pop().unwrap();

                    // If there's a caller frame, store return value
                    if let Some(caller_frame) = proc.frames.last_mut() {
                        if let Some(ret_reg) = finished_frame.caller_return_reg {
                            caller_frame.registers[ret_reg] = return_val.clone();
                        }
                    } else {
                        // No caller - process is done
                        return ExecutionResult::Exited(return_val);
                    }

                    continue; // Continue with caller frame
                }

                bytecode[ip].clone()
            };

            // Advance IP before execution (important for jumps)
            if let Some(frame) = proc.frames.last_mut() {
                frame.ip += 1;
            }

            // Execute instruction
            let exec_result = self.execute_instruction(&mut proc, instruction);

            // Consume one reduction
            let budget_exhausted = proc.consume_reduction();

            match exec_result {
                ExecutionResult::Continue => {
                    if budget_exhausted {
                        return ExecutionResult::BudgetExhausted;
                    }
                    // Continue to next instruction
                }
                ExecutionResult::Yield => {
                    return ExecutionResult::Yield;
                }
                ExecutionResult::Blocked => {
                    // For blocked instructions, revert IP advancement so we retry
                    if let Some(frame) = proc.frames.last_mut() {
                        frame.ip -= 1;
                    }
                    proc.status = ProcessStatus::WaitingForMessage;
                    return ExecutionResult::Blocked;
                }
                other => return other,
            }
        }

        ExecutionResult::BudgetExhausted
    }

    /// Execute a single instruction
    fn execute_instruction(
        &mut self,
        proc: &mut crate::value::Process,
        instruction: Instruction,
    ) -> ExecutionResult {
        match instruction {
            // --- COLOR PALETTE ---
            // ObjectInit: Orange, LoadConst: Gold, Move: LightSkyBlue, Add: LawnGreen, Sub: Tomato, Mul: Violet, Div: LightSalmon
            // Return: SlateBlue, GetProp: LightSeaGreen, SetProp: LightCoral
            // Receive: DodgerBlue, ReceiveWithTimeout: Cyan, Jump: LightPink, JumpIfTrue: MediumSpringGreen, JumpIfFalse: LightYellow
            // Call: DeepSkyBlue, Yield: LightGray, Spawn: MediumPurple, Send: LightSteelBlue, Link: LightSlateGray
            // Match: LightGoldenRodYellow, Nop: Gray, Equal: LightGreen, NotEqual: LightPink, LessThan: LightCyan, LessEqual: LightBlue, GreaterThan: LightSalmon, GreaterEqual: LightSkyBlue
            // And: MediumAquamarine, Or: LightGoldenRodYellow, Not: LightSlateBlue
            Instruction::ObjectInit(dst, kvs) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let mut obj = crate::value::Object::new(None);
                    for (key, arg) in kvs {
                        use crate::value::ObjectInitArg;
                        use crate::value::PropertyDescriptor;
                        match arg {
                            ObjectInitArg::Register(reg) => {
                                obj.properties.insert(
                                    key,
                                    PropertyDescriptor {
                                        value: frame.registers[reg].clone(),
                                        writable: true,
                                        enumerable: false,
                                        configurable: true,
                                    },
                                );
                            }
                            ObjectInitArg::Value(val) => {
                                obj.properties.insert(
                                    key,
                                    PropertyDescriptor {
                                        value: val.clone(),
                                        writable: true,
                                        enumerable: false,
                                        configurable: true,
                                    },
                                );
                            }
                            ObjectInitArg::RegisterWithFlags(reg, w, e, c) => {
                                obj.properties.insert(
                                    key,
                                    PropertyDescriptor {
                                        value: frame.registers[reg].clone(),
                                        writable: w,
                                        enumerable: e,
                                        configurable: c,
                                    },
                                );
                            }
                            ObjectInitArg::ValueWithFlags(val, w, e, c) => {
                                obj.properties.insert(
                                    key,
                                    PropertyDescriptor {
                                        value: val.clone(),
                                        writable: w,
                                        enumerable: e,
                                        configurable: c,
                                    },
                                );
                            }
                        }
                    }
                    frame.registers[dst] =
                        Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)));
                    if self.debug {
                        // Orange
                        println!(
                            "\x1b[38;2;255;165;0m[VM DEBUG]\x1b[0m OBJECT_INIT: Created object in r{} with properties: {:?}",
                            dst, frame.registers[dst]
                        );
                    }
                }
                ExecutionResult::Continue
            }
            Instruction::LoadConst(reg, val) => {
                if let Some(frame) = proc.frames.last_mut() {
                    // Handle special __vm: values
                    let resolved_val = self.resolve_vm_value(val, proc.pid, frame);
                    frame.registers[reg] = resolved_val.clone();
                    if self.debug {
                        // Gold
                        println!(
                            "\x1b[38;2;255;215;0m[VM DEBUG]\x1b[0m LOAD_CONST: Loaded {:?} into r{}",
                            resolved_val, reg
                        );
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Move(dst, src) => {
                if let Some(frame) = proc.frames.last_mut() {
                    frame.registers[dst] = frame.registers[src].clone();
                    if self.debug {
                        // LightSkyBlue
                        println!(
                            "\x1b[38;2;135;206;250m[VM DEBUG]\x1b[0m MOVE: r{} <- r{} ({:?})",
                            dst, src, frame.registers[dst]
                        );
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Add(dst, a, b) => {
                if let Some(frame) = proc.frames.last_mut() {
                    match (&frame.registers[a], &frame.registers[b]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Number(x + y));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(cx)),
                            Value::Primitive(crate::value::Primitive::Complex(cy)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(cx + cy));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Number(n)),
                            Value::Primitive(crate::value::Primitive::Complex(c)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(n + c));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(c)),
                            Value::Primitive(crate::value::Primitive::Number(n)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(c + n));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::String(sx)),
                            Value::Primitive(crate::value::Primitive::String(sy)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::String(sx.clone() + sy));
                        }
                        _ => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                    if self.debug {
                        // LawnGreen
                        println!(
                            "\x1b[38;2;124;252;0m[VM DEBUG]\x1b[0m ADD: r{} = r{} + r{} -> {:?}",
                            dst, a, b, frame.registers[dst]
                        );
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Sub(dst, a, b) => {
                if let Some(frame) = proc.frames.last_mut() {
                    match (&frame.registers[a], &frame.registers[b]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Number(x - y));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(cx)),
                            Value::Primitive(crate::value::Primitive::Complex(cy)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(cx - cy));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Number(n)),
                            Value::Primitive(crate::value::Primitive::Complex(c)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(n - c));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(c)),
                            Value::Primitive(crate::value::Primitive::Number(n)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(c - n));
                        }
                        _ => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                    if self.debug {
                        // Tomato
                        println!(
                            "\x1b[38;2;255;99;71m[VM DEBUG]\x1b[0m SUB: r{} = r{} - r{} -> {:?}",
                            dst, a, b, frame.registers[dst]
                        );
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Mul(dst, a, b) => {
                if let Some(frame) = proc.frames.last_mut() {
                    match (&frame.registers[a], &frame.registers[b]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Number(x * y));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(cx)),
                            Value::Primitive(crate::value::Primitive::Complex(cy)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(cx * cy));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(c)),
                            Value::Primitive(crate::value::Primitive::Number(n)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(c * n));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Number(n)),
                            Value::Primitive(crate::value::Primitive::Complex(c)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(n * c));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::String(sx)),
                            Value::Primitive(crate::value::Primitive::Number(n)),
                        ) => {
                            // String multiplication with number (repeat string)
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::String(sx.repeat(*n as usize)));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Number(n)),
                            Value::Primitive(crate::value::Primitive::String(sx)),
                        ) => {
                            // Number multiplication with string (repeat string)
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::String(sx.repeat(*n as usize)));
                        }
                        _ => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                    if self.debug {
                        // Violet
                        println!(
                            "\x1b[38;2;238;130;238m[VM DEBUG]\x1b[0m MUL: r{} = r{} * r{} -> {:?}",
                            dst, a, b, frame.registers[dst]
                        );
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Div(dst, a, b) => {
                if let Some(frame) = proc.frames.last_mut() {
                    match (&frame.registers[a], &frame.registers[b]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) => {
                            if *y != 0.0 {
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Number(x / y));
                            } else {
                                // Division by zero - return Undefined or could be Error
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Undefined);
                            }
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(cx)),
                            Value::Primitive(crate::value::Primitive::Complex(cy)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(cx / cy));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Complex(cy)),
                        ) => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Complex(x / *cy));
                        }
                        (
                            Value::Primitive(crate::value::Primitive::Complex(cx)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) => {
                            if *y != 0.0 {
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Complex(cx / *y));
                            } else {
                                // Division by zero - return Undefined or could be Error
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Undefined);
                            }
                        }
                        _ => {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                    if self.debug {
                        // LightSalmon
                        println!(
                            "\x1b[38;2;255;160;122m[VM DEBUG]\x1b[0m DIV: r{} = r{} / r{} -> {:?}",
                            dst, a, b, frame.registers[dst]
                        );
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Return(reg) => {
                let return_val = if let Some(frame) = proc.frames.last() {
                    frame.registers[reg].clone()
                } else {
                    Value::Primitive(crate::value::Primitive::Unit)
                };
                // Set return value in current frame
                if let Some(frame) = proc.frames.last_mut() {
                    frame.return_value = Some(return_val.clone());
                    if self.debug {
                        // SlateBlue
                        println!(
                            "\x1b[38;2;106;90;205m[VM DEBUG]\x1b[0m RETURN: r{} -> {:?}",
                            reg, return_val
                        );
                    }
                }
                // Check if this is the last frame (main function)
                let is_main_function = proc.frames.len() == 1;
                if is_main_function {
                    // This is the main function returning - mark process as completed
                    proc.alive = false;
                    proc.status = ProcessStatus::Exited;
                    proc.last_result = Some(return_val);
                } else {
                    // This is a nested function returning - get caller info then pop frame
                    let caller_return_reg = proc.frames.last().unwrap().caller_return_reg;
                    proc.frames.pop(); // Remove the current frame
                    // Store return value in caller's register if specified
                    if let (Some(caller_reg), Some(caller_frame)) =
                        (caller_return_reg, proc.frames.last_mut())
                    {
                        caller_frame.registers[caller_reg] = return_val;
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::GetProp(dst, obj_reg, key_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[obj_reg], &frame.registers[key_reg]) {
                        (
                            Value::Object(obj_rc),
                            Value::Primitive(crate::value::Primitive::Atom(key)),
                        ) => {
                            let obj = obj_rc.borrow();
                            obj.get_property(key)
                                .unwrap_or(Value::Primitive(crate::value::Primitive::Undefined))
                        }
                        (
                            Value::Primitive(Primitive::Atom(this)),
                            Value::Primitive(Primitive::Atom(key))
                        ) if this=="__vm:this" => {
                            if let Some(Value::Object(this)) = &frame.function.bound_this {
                                this.borrow().get_this_property(key).unwrap_or(Value::Primitive(crate::Primitive::Undefined))
                            }
                            else {
                                panic!("this is not bound")
                            }
                        }
                        _ => Value::Primitive(crate::value::Primitive::Undefined),
                    };
                    //set bound this if the taken value is a function
                    if let Value::Function(func) = &result {
                        func.borrow_mut()
                            .set_bound_this(
                                frame.registers[obj_reg].clone(),
                            );
                    }
                    frame.registers[dst] = result;
                }
                ExecutionResult::Continue
            }

            Instruction::SetProp(obj_reg, key_reg, val_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    match (&frame.registers[obj_reg], &frame.registers[key_reg]) {
                        (
                            Value::Object(obj_rc),
                            Value::Primitive(crate::value::Primitive::Atom(key)),
                        ) => {
                            let value = frame.registers[val_reg].clone();
                            obj_rc.borrow_mut().set_property(key, value);
                        }
                        _ => {
                            // Invalid object or key type - ignore or could be error
                        }
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Receive(dst) => {
                if self.debug {
                    // DodgerBlue
                    println!(
                        "\x1b[38;2;30;144;255m[VM DEBUG]\x1b[0m RECEIVE: Process {} trying to receive into r{}",
                        proc.pid, dst
                    );
                    println!(
                        "\x1b[38;2;30;144;255m[VM DEBUG]\x1b[0m RECEIVE: Mailbox size: {}",
                        proc.mailbox.len()
                    );
                }
                if let Some(msg) = proc.mailbox.pop() {
                    if self.debug {
                        // SpringGreen
                        println!(
                            "\x1b[38;2;0;255;127m[VM DEBUG]\x1b[0m RECEIVE: Got message: {:?}",
                            msg
                        );
                    }
                    if let Some(frame) = proc.frames.last_mut() {
                        frame.registers[dst] = msg;
                        if self.debug {
                            // MediumOrchid
                            println!(
                                "\x1b[38;2;186;85;211m[VM DEBUG]\x1b[0m RECEIVE: Stored message in r{}",
                                dst
                            );
                        }
                    }
                    ExecutionResult::Continue
                } else {
                    // No message available - block process
                    if self.debug {
                        // Crimson
                        println!(
                            "\x1b[38;2;220;20;60m[VM DEBUG]\x1b[0m RECEIVE: No message available, blocking"
                        );
                    }
                    ExecutionResult::Blocked
                }
            }

            Instruction::ReceiveWithTimeout(dst, timeout_reg, result_reg) => {
                if self.debug {
                    println!(
                        "\x1b[36m[VM DEBUG]\x1b[0m RECEIVE_WITH_TIMEOUT: Process {} trying to receive into r{} with timeout from r{}, result to r{}",
                        proc.pid, dst, timeout_reg, result_reg
                    );
                    println!(
                        "\x1b[36m[VM DEBUG]\x1b[0m RECEIVE_WITH_TIMEOUT: Mailbox size: {}",
                        proc.mailbox.len()
                    );
                }
                if let Some(msg) = proc.mailbox.pop() {
                    // Remove any pending timeouts for this process (since message was received)
                    self.pending_timeouts.retain(|t| t.pid != proc.pid);
                    if self.debug {
                        println!(
                            "\x1b[36m[VM DEBUG]\x1b[0m RECEIVE_WITH_TIMEOUT: Got message: {:?}",
                            msg
                        );
                    }
                    if let Some(frame) = proc.frames.last_mut() {
                        frame.registers[dst] = msg;
                        frame.registers[result_reg] =
                            Value::Primitive(crate::value::Primitive::Boolean(true));
                        if self.debug {
                            println!(
                                "\x1b[36m[VM DEBUG]\x1b[0m RECEIVE_WITH_TIMEOUT: Stored message in r{}, set result r{} to true",
                                dst, result_reg
                            );
                        }
                    }
                    ExecutionResult::Continue
                } else {
                    // Get timeout value from register (in milliseconds)
                    let frame_index = proc.frames.len().saturating_sub(1);
                    if let Some(frame) = proc.frames.last() {
                        if let Value::Primitive(crate::value::Primitive::Number(timeout_ms)) =
                            &frame.registers[timeout_reg]
                        {
                            let current_time = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            let expiry_time = current_time + (*timeout_ms as u64);
                            // Store frame index in TimeoutInfo
                            self.pending_timeouts.push(TimeoutInfo {
                                pid: proc.pid,
                                dst_reg: dst,
                                result_reg: result_reg,
                                expiry_ms: expiry_time,
                                frame_index,
                            });
                            if self.debug {
                                println!(
                                    "\x1b[36m[VM DEBUG]\x1b[0m RECEIVE_WITH_TIMEOUT: No message available, set timeout for {}ms (expires at {}), frame_index {}",
                                    timeout_ms, expiry_time, frame_index
                                );
                            }
                            proc.status = ProcessStatus::WaitingForMessage;
                            ExecutionResult::Blocked
                        } else {
                            ExecutionResult::Error("Timeout value must be a number".to_string())
                        }
                    } else {
                        ExecutionResult::Error(
                            "No frame available for timeout operation".to_string(),
                        )
                    }
                }
            }

            Instruction::Jump(offset) => {
                if let Some(frame) = proc.frames.last_mut() {
                    // Since we already incremented IP, we need to adjust
                    // offset is relative to the original instruction position
                    let new_ip = (frame.ip as isize + offset - 1) as usize;
                    frame.ip = new_ip;
                }
                ExecutionResult::Continue
            }

            Instruction::JumpIfTrue(cond_reg, offset) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let condition = &frame.registers[cond_reg];
                    let should_jump = match condition {
                        Value::Primitive(crate::value::Primitive::Boolean(true)) => true,
                        _ => self.is_truthy(condition), // Most values are truthy
                    };

                    if should_jump {
                        let new_ip = (frame.ip as isize + offset - 1) as usize;
                        if self.debug {
                            println!(
                                "[VM DEBUG] JUMPIFTRUE: Pattern matched, jumping to IP {} from {}",
                                new_ip, frame.ip
                            );
                        }
                        frame.ip = new_ip;
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::JumpIfFalse(cond_reg, offset) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let condition = &frame.registers[cond_reg];
                    let should_jump = match condition {
                        Value::Primitive(crate::value::Primitive::Boolean(false)) => true,
                        _ => !self.is_truthy(condition), // Most values are truthy
                    };

                    if should_jump {
                        let new_ip = (frame.ip as isize + offset - 1) as usize;
                        if self.debug {
                            println!("[VM DEBUG] JUMPIFFALSE: Checking condition {:?}", condition);
                            println!(
                                "[VM DEBUG] JUMPIFFALSE: Pattern matched, jumping to IP {} from {}",
                                new_ip, frame.ip
                            );
                        }
                        frame.ip = new_ip;
                    }
                }
                ExecutionResult::Continue
            }

            Instruction::Call(dst_reg, func_reg, arg_regs) => {
                // First, collect the function and arguments
                let (func_value, args) = {
                    let frame = proc.frames.last().unwrap();
                    let func_value = frame.registers[func_reg].clone();
                    let args: Vec<Value> = arg_regs
                        .iter()
                        .map(|&reg| frame.registers[reg].clone())
                        .collect();
                    (func_value, args)
                };
                match func_value {
                    Value::Function(func_rc) => {
                        match &func_rc.borrow().function_type {
                            crate::value::FunctionType::Bytecode { bytecode: _ } => {
                                // Regular bytecode function call
                                let mut new_registers = args;
                                // Ensure we have enough registers for the function's needs
                                let total_regs = func_rc.borrow().total_registers().max(16); // Minimum 16 registers for compatibility
                                new_registers.resize(
                                    total_regs,
                                    Value::Primitive(crate::value::Primitive::Undefined),
                                );

                                let new_frame = Frame {
                                    registers: new_registers,
                                    stack: Vec::new(),
                                    ip: 0,
                                    function: Rc::new(func_rc.borrow().clone()),
                                    return_value: None,
                                    caller_return_reg: Some(dst_reg),
                                };

                                proc.frames.push(new_frame);
                                ExecutionResult::Continue
                            }

                            crate::value::FunctionType::Ffi { function_name } => {
                                // FFI function call - execute immediately
                                let result =
                                    call_ffi_function(&self.ffi_registry, function_name, args);

                                if let Some(frame) = proc.frames.last_mut() {
                                    match result {
                                        FfiCallResult::Success(value) => {
                                            frame.registers[dst_reg] = value;
                                        }
                                        FfiCallResult::Error(err_msg) => {
                                            frame.registers[dst_reg] =
                                                Value::Primitive(crate::value::Primitive::Atom(
                                                    format!("Error: {}", err_msg),
                                                ));
                                        }
                                    }
                                }

                                ExecutionResult::Continue
                            }
                        }
                    }

                    Value::Closure(closure_rc) => {
                        // For closures, merge environment with arguments
                        let mut new_registers: Vec<Value> =
                            closure_rc.environment.values().cloned().collect();
                        new_registers.extend(args);
                        // Ensure we have enough registers for the function's needs
                        let total_regs = closure_rc.function.total_registers().max(16); // Minimum 16 registers for compatibility
                        new_registers.resize(
                            total_regs,
                            Value::Primitive(crate::value::Primitive::Undefined),
                        );

                        let new_frame = Frame {
                            registers: new_registers,
                            stack: Vec::new(),
                            ip: 0,
                            function: closure_rc.function.clone(),
                            return_value: None,
                            caller_return_reg: Some(dst_reg),
                        };

                        proc.frames.push(new_frame);
                        ExecutionResult::Continue
                    }

                    _ => {
                        // Not a callable - set result to Undefined and continue
                        panic!("Attempted to call non-function value: {:?}", func_value);
                        // if let Some(frame) = proc.frames.last_mut() {
                        //     frame.registers[dst_reg] =
                        //         Value::Primitive(crate::value::Primitive::Undefined);
                        // }
                        // ExecutionResult::Continue
                    }
                }
            }

            Instruction::Yield => ExecutionResult::Yield,

            Instruction::Spawn(dst_reg, func_reg, arg_regs) => {
                // First, collect the function and arguments
                let (func_value, args) = {
                    let frame = proc.frames.last().unwrap();
                    let func = frame.registers[func_reg].clone();
                    let mut arguments = Vec::new();
                    for arg_reg in arg_regs {
                        arguments.push(frame.registers[arg_reg].clone());
                    }
                    (func, arguments)
                };

                // Spawn based on function type
                match func_value {
                    Value::Function(func_rc_refcell) => {
                        let func_rc = func_rc_refcell.borrow();
                        if self.debug {
                            // Debug: Log spawn arguments
                            if self.debug {
                                println!(
                                    "[VM DEBUG] SPAWN: Function {:?} with {} args",
                                    func_rc.name,
                                    args.len()
                                );
                                for (i, arg) in args.iter().enumerate() {
                                    match arg {
                                        Value::Process(proc_ref) => {
                                            if let Ok(proc) = proc_ref.try_borrow() {
                                                println!(
                                                    "[VM DEBUG] SPAWN: Arg {}: Process(pid: {})",
                                                    i, proc.pid
                                                );
                                            } else {
                                                println!(
                                                    "[VM DEBUG] SPAWN: Arg {}: Process(borrowed)",
                                                    i
                                                );
                                            }
                                        }
                                        _ => {
                                            println!("[VM DEBUG] SPAWN: Arg {}: {:?}", i, arg);
                                        }
                                    }
                                }
                            }
                        }

                        // Spawn a new process with this function
                        let new_pid =
                            self.spawn_process(Rc::new(func_rc_refcell.borrow().clone()), args);

                        // Store the process reference in the destination register
                        if let Some(new_process) = self.processes.get(&new_pid) {
                            if let Some(frame) = proc.frames.last_mut() {
                                frame.registers[dst_reg] = Value::Process(new_process.clone());
                            }
                        }
                        ExecutionResult::Continue
                    }
                    _ => {
                        // Not a function - can't spawn
                        eprintln!(
                            "[VM DEBUG] SPAWN: Attempted to spawn non-function value: {:?}",
                            func_value
                        );
                        if let Some(frame) = proc.frames.last_mut() {
                            frame.registers[dst_reg] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                        ExecutionResult::Continue
                    }
                }
            }

            Instruction::Send(proc_reg, msg_reg) => {
                let (target_proc, message) = {
                    let frame = proc.frames.last().unwrap();
                    (
                        frame.registers[proc_reg].clone(),
                        frame.registers[msg_reg].clone(),
                    )
                };

                if self.debug {
                    println!(
                        "[VM DEBUG] SEND: From process {} to register r{}",
                        proc.pid, proc_reg
                    );
                    println!("[VM DEBUG] SEND: Message in r{}: {:?}", msg_reg, message);
                }

                match target_proc {
                    Value::Process(proc_rc) => {
                        let target_pid = proc_rc.borrow().pid;
                        if self.debug {
                            if self.debug {
                                println!("[VM DEBUG] SEND: Sending to process {}", target_pid);
                            }
                        }

                        // Send message to target process mailbox
                        proc_rc.borrow_mut().mailbox.push(message);
                        if self.debug {
                            if self.debug {
                                println!(
                                    "[VM DEBUG] SEND: Message added to process {} mailbox (size: {})",
                                    target_pid,
                                    proc_rc.borrow().mailbox.len()
                                );
                            }
                        }

                        // If target was waiting for messages, make it runnable and remove any pending timeouts
                        let target_status = proc_rc.borrow().status.clone();
                        if self.debug {
                            println!(
                                "[VM DEBUG] SEND: Target process {} status: {:?}",
                                target_pid, target_status
                            );
                        }
                        if target_status == ProcessStatus::WaitingForMessage {
                            proc_rc.borrow_mut().status = ProcessStatus::Runnable;
                            // Remove any pending timeouts for this process
                            self.pending_timeouts.retain(|t| t.pid != target_pid);
                            if self.debug {
                                println!(
                                    "[VM DEBUG] SEND: Changed process {} status to Runnable and removed pending timeouts",
                                    target_pid
                                );
                            }
                            // Add to run queue if not already there
                            if !self.run_queue.contains(&target_pid) {
                                self.run_queue.push_back(target_pid);
                                if self.debug {
                                    println!(
                                        "[VM DEBUG] SEND: Added process {} to run queue",
                                        target_pid
                                    );
                                }
                            } else if self.debug {
                                println!(
                                    "[VM DEBUG] SEND: Process {} already in run queue",
                                    target_pid
                                );
                            }
                        }
                        ExecutionResult::Continue
                    }

                    _ => {
                        if self.debug {
                            if self.debug {
                                println!("[VM DEBUG] SEND: Target is not a process, ignoring");
                            }
                        }
                        ExecutionResult::Continue
                    }
                }
            }

            Instruction::Link(proc_reg) => {
                let target_proc = {
                    let frame = proc.frames.last().unwrap();
                    frame.registers[proc_reg].clone()
                };

                match target_proc {
                    Value::Process(target_proc_rc) => {
                        let current_pid = proc.pid;
                        let target_pid = target_proc_rc.borrow().pid;

                        // Create bidirectional link
                        // Add target to current process's links
                        if !proc.links.contains(&target_pid) {
                            proc.links.push(target_pid);
                        }

                        // Add current to target process's links
                        let mut target_proc_borrow = target_proc_rc.borrow_mut();
                        if !target_proc_borrow.links.contains(&current_pid) {
                            target_proc_borrow.links.push(current_pid);
                        }

                        ExecutionResult::Continue
                    }

                    _ => {
                        // Not a process - can't link
                        ExecutionResult::Continue
                    }
                }
            }

            Instruction::Match(src_reg, patterns) => {
                let value = {
                    let frame = proc.frames.last().unwrap();
                    frame.registers[src_reg].clone()
                };

                // Try to match patterns in order
                for (pattern, jump_offset) in patterns {
                    if self.matches_pattern(&value, &pattern) {
                        // Pattern matched - jump to the corresponding offset
                        if let Some(frame) = proc.frames.last_mut() {
                            let new_ip = (frame.ip as isize + jump_offset - 1) as usize;
                            if self.debug {
                                println!(
                                    "[VM DEBUG] MATCH: Pattern matched, jumping to IP {} from {}",
                                    new_ip, frame.ip
                                );
                            }
                            frame.ip = new_ip;
                        }
                        return ExecutionResult::Continue;
                    }
                }

                // No pattern matched - continue to next instruction
                ExecutionResult::Continue
            }

            Instruction::Nop => ExecutionResult::Continue,

            // Comparison operations
            Instruction::Equal(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[a_reg], &frame.registers[b_reg]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(a)),
                            Value::Primitive(crate::value::Primitive::Number(b)),
                        ) => a == b,
                        (
                            Value::Primitive(crate::value::Primitive::Boolean(a)),
                            Value::Primitive(crate::value::Primitive::Boolean(b)),
                        ) => a == b,
                        (
                            Value::Primitive(crate::value::Primitive::String(a)),
                            Value::Primitive(crate::value::Primitive::String(b)),
                        ) => a == b,
                        (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::Atom(b)),
                        ) => a == b,
                        (
                            Value::Primitive(crate::value::Primitive::String(s)),
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                        )
                        | (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::String(s)),
                        ) => s == a,
                        (
                            Value::Primitive(crate::value::Primitive::Unit),
                            Value::Primitive(crate::value::Primitive::Unit),
                        ) => true,
                        (
                            Value::Primitive(crate::value::Primitive::Undefined),
                            Value::Primitive(crate::value::Primitive::Undefined),
                        ) => true,
                        _ => false,
                    };
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(result));
                }
                ExecutionResult::Continue
            }

            Instruction::NotEqual(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[a_reg], &frame.registers[b_reg]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(a)),
                            Value::Primitive(crate::value::Primitive::Number(b)),
                        ) => a != b,
                        (
                            Value::Primitive(crate::value::Primitive::Boolean(a)),
                            Value::Primitive(crate::value::Primitive::Boolean(b)),
                        ) => a != b,
                        (
                            Value::Primitive(crate::value::Primitive::String(a)),
                            Value::Primitive(crate::value::Primitive::String(b)),
                        ) => a != b,
                        (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::Atom(b)),
                        ) => a != b,
                        (
                            Value::Primitive(crate::value::Primitive::String(s)),
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                        )
                        | (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::String(s)),
                        ) => s != a,
                        (
                            Value::Primitive(crate::value::Primitive::Unit),
                            Value::Primitive(crate::value::Primitive::Unit),
                        ) => false,
                        (
                            Value::Primitive(crate::value::Primitive::Undefined),
                            Value::Primitive(crate::value::Primitive::Undefined),
                        ) => false,
                        _ => true,
                    };
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(result));
                }
                ExecutionResult::Continue
            }

            Instruction::LessThan(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[a_reg], &frame.registers[b_reg]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(a)),
                            Value::Primitive(crate::value::Primitive::Number(b)),
                        ) => a < b,
                        (
                            Value::Primitive(crate::value::Primitive::String(a)),
                            Value::Primitive(crate::value::Primitive::String(b)),
                        ) => a < b,
                        (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::Atom(b)),
                        ) => a < b,
                        _ => false,
                    };
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(result));
                }
                ExecutionResult::Continue
            }

            Instruction::LessEqual(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[a_reg], &frame.registers[b_reg]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(a)),
                            Value::Primitive(crate::value::Primitive::Number(b)),
                        ) => a <= b,
                        (
                            Value::Primitive(crate::value::Primitive::String(a)),
                            Value::Primitive(crate::value::Primitive::String(b)),
                        ) => a <= b,
                        (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::Atom(b)),
                        ) => a <= b,
                        _ => false,
                    };
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(result));
                }
                ExecutionResult::Continue
            }

            Instruction::GreaterThan(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[a_reg], &frame.registers[b_reg]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(a)),
                            Value::Primitive(crate::value::Primitive::Number(b)),
                        ) => a > b,
                        (
                            Value::Primitive(crate::value::Primitive::String(a)),
                            Value::Primitive(crate::value::Primitive::String(b)),
                        ) => a > b,
                        (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::Atom(b)),
                        ) => a > b,
                        _ => false,
                    };
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(result));
                }
                ExecutionResult::Continue
            }

            Instruction::GreaterEqual(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let result = match (&frame.registers[a_reg], &frame.registers[b_reg]) {
                        (
                            Value::Primitive(crate::value::Primitive::Number(a)),
                            Value::Primitive(crate::value::Primitive::Number(b)),
                        ) => a >= b,
                        (
                            Value::Primitive(crate::value::Primitive::String(a)),
                            Value::Primitive(crate::value::Primitive::String(b)),
                        ) => a >= b,
                        (
                            Value::Primitive(crate::value::Primitive::Atom(a)),
                            Value::Primitive(crate::value::Primitive::Atom(b)),
                        ) => a >= b,
                        _ => false,
                    };
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(result));
                }
                ExecutionResult::Continue
            }

            // Logical operations
            Instruction::And(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let a_truthy = self.is_truthy(&frame.registers[a_reg]);
                    let b_truthy = self.is_truthy(&frame.registers[b_reg]);
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(a_truthy && b_truthy));
                }
                ExecutionResult::Continue
            }

            Instruction::Or(dst, a_reg, b_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let a_truthy = self.is_truthy(&frame.registers[a_reg]);
                    let b_truthy = self.is_truthy(&frame.registers[b_reg]);
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(a_truthy || b_truthy));
                }
                ExecutionResult::Continue
            }

            Instruction::Not(dst, src_reg) => {
                if let Some(frame) = proc.frames.last_mut() {
                    let truthy = self.is_truthy(&frame.registers[src_reg]);
                    frame.registers[dst] =
                        Value::Primitive(crate::value::Primitive::Boolean(!truthy));
                }
                ExecutionResult::Continue
            }
        }
    }

    /// Check if a value matches a pattern
    fn matches_pattern(&self, value: &Value, pattern: &Pattern) -> bool {
        match (value, pattern) {
            (_, Pattern::Wildcard) => true,

            (Value::Primitive(val_prim), Pattern::Value(Value::Primitive(pat_prim))) => {
                val_prim == pat_prim
            }

            (Value::Array(arr), Pattern::Array(patterns)) => {
                let arr_borrow = arr.borrow();
                if arr_borrow.len() != patterns.len() {
                    return false;
                }

                for (val, pat) in arr_borrow.iter().zip(patterns.iter()) {
                    if !self.matches_pattern(val, pat) {
                        return false;
                    }
                }
                true
            }

            (Value::Tuple(tuple), Pattern::Tuple(patterns)) => {
                if tuple.len() != patterns.len() {
                    return false;
                }

                for (val, pat) in tuple.iter().zip(patterns.iter()) {
                    if !self.matches_pattern(val, pat) {
                        return false;
                    }
                }
                true
            }

            (Value::TaggedEnum(enum_val), Pattern::TaggedEnum(tag, pattern)) => {
                enum_val.tag == *tag && self.matches_pattern(&enum_val.value, pattern)
            }

            (val, Pattern::Value(pat_val)) => val == pat_val,

            _ => false,
        }
    }

    /// Handle the result of process execution
    fn handle_execution_result(&mut self, pid: usize, result: ExecutionResult) {
        match result {
            ExecutionResult::BudgetExhausted | ExecutionResult::Yield => {
                // Reschedule process
                self.run_queue.push_back(pid);
            }

            ExecutionResult::Blocked => {
                // Process is blocked - don't reschedule until unblocked
                if let Some(proc_ref) = self.processes.get(&pid) {
                    proc_ref.borrow_mut().status = ProcessStatus::WaitingForMessage;
                }
            }

            ExecutionResult::Exited(return_val) => {
                // Process finished
                if let Some(proc_ref) = self.processes.get(&pid) {
                    let mut proc = proc_ref.borrow_mut();
                    proc.alive = false;
                    proc.status = ProcessStatus::Exited;
                    proc.last_result = Some(return_val);
                }
                // Don't reschedule
            }

            ExecutionResult::Error(msg) => {
                eprintln!("Process {} error: {}", pid, msg);
                if let Some(proc_ref) = self.processes.get(&pid) {
                    let mut proc = proc_ref.borrow_mut();
                    proc.alive = false;
                    proc.status = ProcessStatus::Exited;
                }
                // Don't reschedule
            }

            ExecutionResult::Continue => {
                // This shouldn't happen at the top level
                self.run_queue.push_back(pid);
            }
        }
    }

    /// Unblock processes waiting for messages
    pub fn check_blocked_processes(&mut self) {
        for (pid, proc_ref) in &self.processes {
            // Scope to avoid double mutable borrow
            let mut deliver = None;
            {
                let mut proc = proc_ref.borrow_mut();
                if proc.status == ProcessStatus::WaitingForMessage && !proc.mailbox.is_empty() {
                    // Remove any pending timeouts for this process
                    self.pending_timeouts.retain(|t| t.pid != *pid);
                    // Pop the message now to avoid double borrow
                    let msg = proc.mailbox.pop();
                    deliver = Some((proc.frames.len(), msg));
                    proc.status = ProcessStatus::Runnable;
                    self.run_queue.push_back(*pid);
                }
            }
            // Deliver the message and set result register if needed
            if let Some((_frame_count, Some(msg))) = deliver {
                let mut proc = proc_ref.borrow_mut();
                if let Some(frame) = proc.frames.last_mut() {
                    if frame.ip > 0 {
                        let ip = frame.ip - 1;
                        let bytecode = match &frame.function.function_type {
                            crate::value::FunctionType::Bytecode { bytecode } => bytecode,
                            _ => &vec![],
                        };
                        if let Some(crate::vm::Instruction::ReceiveWithTimeout(dst, _timeout_reg, result_reg)) = bytecode.get(ip) {
                            frame.registers[*dst] = msg;
                            frame.registers[*result_reg] = Value::Primitive(crate::value::Primitive::Boolean(true));
                        }
                    }
                }
            }
        }
    }

    /// Check if a value is truthy for logical operations
    fn is_truthy(&self, value: &Value) -> bool {
        match value {
            Value::Primitive(crate::value::Primitive::Boolean(b)) => *b,
            Value::Primitive(crate::value::Primitive::Number(n)) => *n != 0.0,
            Value::Primitive(crate::value::Primitive::String(s)) => !s.is_empty(),
            Value::Primitive(crate::value::Primitive::Atom(a)) => !a.is_empty(),
            Value::Primitive(crate::value::Primitive::Unit) => false,
            Value::Primitive(crate::value::Primitive::Undefined) => false,
            // Other value types are generally truthy
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Function, Primitive, Value};

    fn create_simple_add_function() -> Rc<Function> {
        Rc::new(Function::new_bytecode(
            Some("add".to_string()),
            0,
            3, // extra_regs - uses registers 0, 1, 2
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Number(2.0))),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(3.0))),
                Instruction::Add(2, 0, 1),
                Instruction::Return(2),
            ],
        ))
    }

    #[test]
    fn test_ion_vm_creation() {
        let vm = IonVM::new();
        assert_eq!(vm.processes.len(), 0);
        assert_eq!(vm.run_queue.len(), 0);
        assert_eq!(vm.next_pid, 1);
        assert_eq!(vm.reduction_limit, 2000);
    }

    #[test]
    fn test_spawn_process() {
        let mut vm = IonVM::new();
        let func = create_simple_add_function();
        let pid = vm.spawn_process(func, vec![]);

        assert_eq!(pid, 1);
        assert_eq!(vm.processes.len(), 1);
        assert_eq!(vm.run_queue.len(), 1);
        assert!(vm.processes.contains_key(&pid));
    }

    #[test]
    fn test_simple_execution() {
        let mut vm = IonVM::new();
        let func = create_simple_add_function();
        let pid = vm.spawn_process(func, vec![]);

        vm.run();

        let proc = vm.processes.get(&pid).unwrap();
        let proc_borrow = proc.borrow();
        assert!(!proc_borrow.alive);
        assert_eq!(proc_borrow.status, ProcessStatus::Exited);
        // Should have 2.0 + 3.0 = 5.0 as result
        assert_eq!(
            proc_borrow.last_result,
            Some(Value::Primitive(Primitive::Number(5.0)))
        );
    }

    #[test]
    fn test_reduction_counting() {
        let mut vm = IonVM::new();
        vm.reduction_limit = 2; // Very small budget for testing

        // Create a function with many instructions
        let func = Rc::new(Function::new_bytecode(
            Some("many_ops".to_string()),
            0,
            5, // extra_regs - uses registers 0, 1, 2, 3, 4
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Number(1.0))),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(1.0))),
                Instruction::Add(2, 0, 1), // Should exhaust budget here
                Instruction::Add(3, 2, 0),
                Instruction::Add(4, 3, 1),
                Instruction::Return(4),
            ],
        ));

        let pid = vm.spawn_process(func, vec![]);

        // Process should be preempted due to budget exhaustion
        let proc_ref = vm.processes.get(&pid).unwrap().clone();
        let result = vm.execute_process_slice(proc_ref);

        assert_eq!(result, ExecutionResult::BudgetExhausted);
    }

    #[test]
    fn test_match_instruction() {
        use crate::value::{Function, Primitive, Value};
        use crate::vm::Instruction;
        use crate::vm::Pattern;
        use std::rc::Rc;

        // Function: r0 = 42; match r0 { 42 => jump +2; _ => jump +1 }; r1 = 1; r1 = 2; return r1
        let func = Rc::new(Function::new_bytecode(
            Some("match_test".to_string()),
            0,
            2,
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Atom("abc".to_string()))),
                Instruction::Match(
                    0,
                    vec![
                        (
                            Pattern::Value(Value::Primitive(Primitive::Atom("abc".to_string()))),
                            1,
                        ),
                        (Pattern::Wildcard, 3),
                    ],
                ),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(2.0))), // skipped if match
                Instruction::Jump(2),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(1.0))),
                Instruction::Return(1),
            ],
        ));

        let mut vm = IonVM::new();
        let pid = vm.spawn_process(func, vec![]);
        vm.run();
        let proc = vm.processes.get(&pid).unwrap();
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(2.0)))
        );
    }
    #[test]
    fn test_arithmetic_operations() {
        let mut vm = IonVM::new();

        // Test all arithmetic operations: 10 - 3 * 2 / 2 = 10 - 6 / 2 = 10 - 3 = 7
        let func = Rc::new(Function::new_bytecode(
            Some("arithmetic".to_string()),
            0,
            6, // extra_regs - uses registers 0, 1, 2, 3, 4, 5
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Number(10.0))),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(3.0))),
                Instruction::LoadConst(2, Value::Primitive(Primitive::Number(2.0))),
                Instruction::Mul(3, 1, 2), // r3 = 3 * 2 = 6
                Instruction::Div(4, 3, 2), // r4 = 6 / 2 = 3
                Instruction::Sub(5, 0, 4), // r5 = 10 - 3 = 7
                Instruction::Return(5),
            ],
        ));

        let pid = vm.spawn_process(func, vec![]);
        vm.run();

        let proc = vm.processes.get(&pid).unwrap();
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(7.0)))
        );
    }

    #[test]
    fn test_property_operations() {
        use crate::value::{Object, PropertyDescriptor};

        let mut vm = IonVM::new();

        // Create an object and test property get/set
        let mut obj = Object::new(None);
        obj.properties.insert(
            "x".to_string(),
            PropertyDescriptor {
                value: Value::Primitive(Primitive::Number(42.0)),
                writable: true,
                enumerable: true,
                configurable: true,
            },
        );
        let obj_val = Value::Object(Rc::new(RefCell::new(obj)));

        let func = Rc::new(Function::new_bytecode(
            Some("prop_test".to_string()),
            0,
            3, // extra_regs - uses registers 0, 1, 2
            vec![
                Instruction::LoadConst(0, obj_val),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("x".to_string()))),
                Instruction::GetProp(2, 0, 1), // r2 = obj["x"]
                Instruction::Return(2),
            ],
        ));

        let pid = vm.spawn_process(func, vec![]);
        vm.run();

        let proc = vm.processes.get(&pid).unwrap();
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(42.0)))
        );
    }

    #[test]
    fn test_jump_instructions() {
        let mut vm = IonVM::new();

        // Test conditional jump: if true, skip setting r0 to 999
        let func = Rc::new(Function::new_bytecode(
            Some("jump_test".to_string()),
            0,
            2, // extra_regs - uses registers 0, 1
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Boolean(true))),
                Instruction::JumpIfTrue(0, 2), // Jump 2 instructions ahead if true
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(999.0))), // Should be skipped
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(42.0))), // Should execute
                Instruction::Return(1),
            ],
        ));

        let pid = vm.spawn_process(func, vec![]);
        vm.run();

        let proc = vm.processes.get(&pid).unwrap();
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(42.0)))
        );
    }

    #[test]
    fn test_function_call() {
        let mut vm = IonVM::new();

        // Inner function: add two numbers
        let add_func = Rc::new(RefCell::new(Function::new_bytecode(
            Some("add".to_string()),
            2,
            1, // extra_regs - arity 2 + 1 extra register (for register 2)
            vec![
                Instruction::Add(2, 0, 1), // r2 = r0 + r1 (args)
                Instruction::Return(2),
            ],
        )));

        // Outer function: call add(5, 7)
        let main_func = Rc::new(Function::new_bytecode(
            Some("main".to_string()),
            0,
            4, // extra_regs - uses registers 0, 1, 2, 3
            vec![
                Instruction::LoadConst(0, Value::Function(add_func)),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(5.0))),
                Instruction::LoadConst(2, Value::Primitive(Primitive::Number(7.0))),
                Instruction::Call(3, 0, vec![1, 2]), // r3 = add(r1, r2)
                Instruction::Return(3),
            ],
        ));

        let pid = vm.spawn_process(main_func, vec![]);
        vm.run();

        let proc = vm.processes.get(&pid).unwrap();
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(12.0)))
        );
    }

    #[test]
    fn test_spawn_and_send() {
        let mut vm = IonVM::new();

        // Child process: receive a message and return it
        let child_func = Rc::new(RefCell::new(Function::new_bytecode(
            Some("child".to_string()),
            0,
            1, // extra_regs - uses register 0
            vec![
                Instruction::Receive(0), // r0 = receive message
                Instruction::Return(0),
            ],
        )));

        // Parent process: spawn child, send message, return
        let parent_func = Rc::new(Function::new_bytecode(
            Some("parent".to_string()),
            0,
            4, // extra_regs - uses registers 0, 1, 2, 3
            vec![
                Instruction::LoadConst(0, Value::Function(child_func)),
                Instruction::Spawn(1, 0, vec![]), // r1 = spawn child
                Instruction::LoadConst(2, Value::Primitive(Primitive::Number(123.0))),
                Instruction::Send(1, 2), // send 123 to child
                Instruction::LoadConst(3, Value::Primitive(Primitive::Atom("sent".to_string()))),
                Instruction::Return(3),
            ],
        ));

        let parent_pid = vm.spawn_process(parent_func, vec![]);
        vm.run();

        // Check that parent completed
        let parent_proc = vm.processes.get(&parent_pid).unwrap();
        assert_eq!(
            parent_proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Atom("sent".to_string())))
        );

        // Check that child received the message
        let child_proc = vm
            .processes
            .iter()
            .find(|(pid, _)| **pid != parent_pid)
            .map(|(_, proc)| proc)
            .unwrap();
        assert_eq!(
            child_proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(123.0)))
        );
    }

    #[test]
    fn test_vm_special_values() {
        let mut vm = IonVM::new();

        // Test function that loads various __vm: values
        let test_func = Rc::new(Function::new_bytecode(
            Some("test_vm_values".to_string()),
            0,
            4, // extra_regs - uses registers 0, 1, 2, 3
            vec![
                // Load __vm:self (current process reference)
                Instruction::LoadConst(
                    0,
                    Value::Primitive(Primitive::Atom("__vm:self".to_string())),
                ),
                // Load __vm:pid (current process ID)
                Instruction::LoadConst(
                    1,
                    Value::Primitive(Primitive::Atom("__vm:pid".to_string())),
                ),
                // Load __vm:processes (total process count)
                Instruction::LoadConst(
                    2,
                    Value::Primitive(Primitive::Atom("__vm:processes".to_string())),
                ),
                // Load legacy 'self' (should work too)
                Instruction::LoadConst(3, Value::Primitive(Primitive::Atom("self".to_string()))),
                // Return the PID
                Instruction::Return(1),
            ],
        ));

        let pid = vm.spawn_process(test_func, vec![]);
        vm.run();

        let proc = vm.processes.get(&pid).unwrap();
        let proc_ref = proc.borrow();

        // Check that the function returned the PID
        assert_eq!(
            proc_ref.last_result,
            Some(Value::Primitive(Primitive::Number(pid as f64)))
        );

        // Check that the registers were loaded correctly
        if let Some(frame) = proc_ref.frames.last() {
            // r0 should contain the process reference
            match &frame.registers[0] {
                Value::Process(proc_ref) => {
                    assert_eq!(proc_ref.borrow().pid, pid);
                }
                _ => panic!("Expected process reference in r0"),
            }

            // r1 should contain the PID as a number
            assert_eq!(
                frame.registers[1],
                Value::Primitive(Primitive::Number(pid as f64))
            );

            // r2 should contain the process count
            assert_eq!(
                frame.registers[2],
                Value::Primitive(Primitive::Number(1.0)) // Only one process
            );

            // r3 should contain the process reference (legacy 'self')
            match &frame.registers[3] {
                Value::Process(proc_ref) => {
                    assert_eq!(proc_ref.borrow().pid, pid);
                }
                _ => panic!("Expected process reference in r3"),
            }
        }
    }
    #[test]
fn test_bound_this_method_call() {
    use crate::value::{Function, Object, ObjectInitArg, Primitive, PropertyDescriptor, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    // Function: loads __vm:this and returns it
    let method_func = Rc::new(Function::new_bytecode(
        Some("get_this".to_string()),
        0,
        3, // uses r0 for return
        vec![
            // r0 = __vm:this
            Instruction::LoadConst(0, Value::Primitive(Primitive::Atom("__vm:this".to_string()))),
            //change the name
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("name".to_string()))),
            Instruction::LoadConst(2, Value::Primitive(Primitive::String("New Name".to_string()))),
            Instruction::SetProp(0, 1, 2),
            Instruction::Return(0),
        ],
    ));

    // Create an object and set the method as a property
    let mut obj = Object::new(None);
    obj.properties.insert(
        "get_this".to_string(),
        PropertyDescriptor {
            value: Value::Function(Rc::new(RefCell::new((*method_func).clone()))),
            writable: true,
            enumerable: true,
            configurable: true,
        },
    );
    obj.properties.insert(
        "name".to_string(),
        PropertyDescriptor {
            value: Value::Primitive(Primitive::String("TestObject".to_string())),
            writable: true,
            enumerable: true,
            configurable: true
        }
    );
    let obj_rc = Rc::new(RefCell::new(obj));

    // Function to call the method: r0 = object, r1 = key, r2 = object.get_this(), return r2
    let call_method_func = Rc::new(Function::new_bytecode(
        Some("call_method".to_string()),
        0,
        4,
        vec![
            // r0 = object
            Instruction::LoadConst(0, Value::Object(obj_rc.clone())),
            // r1 = "get_this"
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("get_this".to_string()))),
            // r2 = object["get_this"]
            Instruction::GetProp(2, 0, 1),
            // r3 = call r2()
            Instruction::Call(3, 2, vec![]),
            // return r3
            Instruction::Return(0),
        ],
    ));

    let mut vm = IonVM::new();
    let pid = vm.spawn_process(call_method_func, vec![]);
    vm.run();
    let proc = vm.processes.get(&pid).unwrap().borrow();
    let result = proc.last_result.as_ref().unwrap();
    // The returned value should have its name changed
    if let Value::Object(obj) = result {
        let obj_borrow = obj.borrow();
            assert_eq!(obj_borrow.properties.get("name").unwrap().value,
                Value::Primitive(Primitive::String("New Name".to_string())));
    } else {
        panic!("Expected an object as the result"); 
    }
}
}
