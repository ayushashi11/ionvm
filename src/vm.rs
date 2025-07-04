use crate::value::Process;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

// Stub for pattern_matches
fn pattern_matches(_val: &crate::value::Value, _pattern: &Pattern) -> bool {
    // TODO: implement real pattern matching
    false
}
use std::rc::Rc;

use crate::value::{Function, Value, ProcessStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    Continue,        // Continue execution
    Yield,          // Process yielded voluntarily
    BudgetExhausted, // Reduction budget exhausted
    Blocked,        // Process blocked (e.g., waiting for message)
    Exited(Value),  // Process exited with return value
    Error(String),  // Execution error
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    LoadConst(usize, Value),             // reg, value
    Move(usize, usize),                  // dst, src
    Add(usize, usize, usize),            // dst, a, b
    Sub(usize, usize, usize),            // dst, a, b
    Mul(usize, usize, usize),            // dst, a, b
    Div(usize, usize, usize),            // dst, a, b
    GetProp(usize, usize, usize),        // dst, obj, key
    SetProp(usize, usize, usize),        // obj, key, value
    Call(usize, usize, Vec<usize>),      // dst, func, args
    Return(usize),                       // reg
    Jump(isize),                         // offset
    JumpIfTrue(usize, isize),            // cond_reg, offset
    JumpIfFalse(usize, isize),           // cond_reg, offset
    Spawn(usize, usize, Vec<usize>),     // dst, func, args
    Send(usize, usize),                  // proc, msg
    Receive(usize),                      // dst
    Link(usize),                         // proc
    Match(usize, Vec<(Pattern, isize)>), // src, pattern table (pattern, jump offset)
    Nop,
    // ... more as needed
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Value(Value),
    Wildcard,
    Tuple(Vec<Pattern>),
    Array(Vec<Pattern>),
    TaggedEnum(String, Box<Pattern>),
    // ... more as needed
}

#[derive(Debug)]
pub struct Frame {
    pub registers: Vec<Value>,
    pub stack: Vec<Value>,
    pub ip: usize,
    pub function: Rc<Function>,
    pub return_value: Option<Value>, // Store the return value here
    pub curr_ret_reg: Option<usize>,
}

// Process struct moved to value.rs

#[derive(Debug)]
pub struct VM {
    pub processes: HashMap<usize, Rc<RefCell<Process>>>,
    pub run_queue: VecDeque<usize>,
    pub next_pid: usize,
    pub reduction_limit: u32, // Max reductions per process per scheduling round
}

impl VM {
    pub fn new() -> Self {
        VM {
            processes: HashMap::new(),
            run_queue: VecDeque::new(),
            next_pid: 1,
            reduction_limit: 2000, // Erlang-style: 2000 reductions per scheduling round
        }
    }

    pub fn spawn_process(&mut self, function: Rc<Function>, args: Vec<Value>) -> usize {
        let pid = self.next_pid;
        self.next_pid += 1;
        let process = Rc::new(RefCell::new(crate::value::Process::new(
            pid, function, args,
        )));
        self.processes.insert(pid, process);
        self.run_queue.push_back(pid);
        pid
    }

    pub fn schedule(&mut self) -> Option<Rc<RefCell<Process>>> {
        while let Some(pid) = self.run_queue.pop_front() {
            if let Some(proc_ref) = self.processes.get(&pid) {
                if proc_ref.borrow().alive {
                    self.run_queue.push_back(pid); // round-robin
                    return Some(proc_ref.clone());
                }
            }
        }
        None
    }

    // Erlang-style scheduler with reduction counting
    pub fn run(&mut self) {
        loop {
            // Check if any processes are runnable
            if !self.has_runnable_processes() {
                break; // No more work to do
            }

            // Schedule next process
            if let Some(proc_ref) = self.schedule() {
                let result = self.execute_process_slice(proc_ref.clone());
                self.handle_execution_result(proc_ref, result);
            }
        }
    }

    /// Check if there are any runnable processes
    fn has_runnable_processes(&self) -> bool {
        self.processes.values().any(|p| p.borrow().is_schedulable())
    }

    /// Execute a process for up to reduction_limit instructions
    fn execute_process_slice(&mut self, proc_ref: Rc<RefCell<Process>>) -> ExecutionResult {
        let mut proc = proc_ref.borrow_mut();
        
        // Reset reduction budget
        proc.reset_reductions(self.reduction_limit);
        
        while proc.reductions > 0 && proc.is_schedulable() {
            // Check if process has frames to execute
            if proc.frames.is_empty() {
                proc.status = ProcessStatus::Exited;
                return ExecutionResult::Exited(Value::Primitive(crate::value::Primitive::Unit));
            }

            // Get current instruction
            let (ip, instruction) = {
                let frame = proc.frames.last().unwrap();
                let ip = frame.ip;
                if ip >= frame.function.bytecode.len() {
                    // End of function, handle return
                    let return_val = frame.return_value.clone()
                        .unwrap_or(Value::Primitive(crate::value::Primitive::Unit));
                    return ExecutionResult::Exited(return_val);
                }
                (ip, frame.function.bytecode[ip].clone())
            };

            // Advance IP before execution
            if let Some(frame) = proc.frames.last_mut() {
                frame.ip += 1;
            }

            // Execute instruction and consume reduction
            let exec_result = self.execute_instruction(&mut proc, instruction);
            let budget_exhausted = proc.consume_reduction();
            
            match exec_result {
                ExecutionResult::Continue => {
                    if budget_exhausted {
                        return ExecutionResult::BudgetExhausted;
                    }
                    // Continue to next instruction
                }
                other => return other,
            }
        }

        ExecutionResult::BudgetExhausted
    }

    /// Execute a single instruction
    fn execute_instruction(&mut self, proc: &mut Process, instruction: Instruction) -> ExecutionResult {
        match instruction {
            Instruction::LoadConst(reg, val) => {
                if let Some(frame) = proc.frames.last_mut() {
                    frame.registers[reg] = val;
                }
                ExecutionResult::Continue
            }
            Instruction::Move(dst, src) => {
                if let Some(frame) = proc.frames.last_mut() {
                    frame.registers[dst] = frame.registers[src].clone();
                }
                ExecutionResult::Continue
            }
                Instruction::Add(dst, a, b) => {
                    if let Some(frame) = proc.frames.last_mut() {
                        println!("Adding {} and {}", a, b);
                        if let (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) = (&frame.registers[a], &frame.registers[b])
                        {
                            println!("Adding {} and {}", x, y);
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Number(x + y));
                        } else {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                }
                Instruction::Sub(dst, a, b) => {
                    if let Some(frame) = proc.frames.last_mut() {
                        if let (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) = (&frame.registers[a], &frame.registers[b])
                        {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Number(x - y));
                        } else {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                }
                Instruction::Mul(dst, a, b) => {
                    if let Some(frame) = proc.frames.last_mut() {
                        if let (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) = (&frame.registers[a], &frame.registers[b])
                        {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Number(x * y));
                        } else {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                }
                Instruction::Div(dst, a, b) => {
                    if let Some(frame) = proc.frames.last_mut() {
                        if let (
                            Value::Primitive(crate::value::Primitive::Number(x)),
                            Value::Primitive(crate::value::Primitive::Number(y)),
                        ) = (&frame.registers[a], &frame.registers[b])
                        {
                            if *y != 0.0 {
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Number(x / y));
                            } else {
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Undefined);
                            }
                        } else {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                }
                Instruction::GetProp(dst, obj_reg, key_reg) => {
                    let val = {
                        if let Some(frame) = proc.frames.last() {
                            let obj_val = &frame.registers[obj_reg];
                            let key_val = &frame.registers[key_reg];
                            if let (
                                Value::Object(obj_rc),
                                Value::Primitive(crate::value::Primitive::Atom(key)),
                            ) = (obj_val, key_val)
                            {
                                let obj = obj_rc.borrow();
                                obj.get_property(key)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    if let Some(frame) = proc.frames.last_mut() {
                        frame.registers[dst] =
                            val.unwrap_or(Value::Primitive(crate::value::Primitive::Undefined));
                    }
                }
                Instruction::SetProp(obj_reg, key_reg, val_reg) => {
                    if let Some(frame) = proc.frames.last() {
                        let obj_val = &frame.registers[obj_reg];
                        let key_val = &frame.registers[key_reg];
                        let value = frame.registers[val_reg].clone();
                        if let (
                            Value::Object(obj_rc),
                            Value::Primitive(crate::value::Primitive::Atom(key)),
                        ) = (obj_val, key_val)
                        {
                            obj_rc.borrow_mut().set_property(key, value);
                        }
                    }
                    // No result register
                }
                Instruction::Call(dst, func_reg, arg_regs) => {
                    let (func_val, args) = if let Some(frame) = proc.frames.last_mut() {
                        let func_val = &frame.registers[func_reg];
                        let args: Vec<Value> = arg_regs
                            .iter()
                            .map(|&i| frame.registers[i].clone())
                            .collect();
                        frame.curr_ret_reg = Some(dst);
                        (func_val.clone(), args)
                    } else {
                        (Value::Primitive(crate::value::Primitive::Undefined), vec![])
                    };
                    match func_val {
                        Value::Function(f) => {
                            let new_frame = crate::vm::Frame {
                                registers: {
                                    let mut regs = args;
                                    regs.resize(
                                        16,
                                        Value::Primitive(crate::value::Primitive::Undefined),
                                    );
                                    regs
                                },
                                stack: Vec::new(),
                                ip: 0,
                                function: f.clone(),
                                return_value: None,
                                curr_ret_reg: None,
                            };
                            proc.frames.push(new_frame);
                        }
                        Value::Closure(c) => {
                            let mut regs: Vec<Value> = c.environment.values().cloned().collect();
                            regs.extend(args);
                            regs.resize(16, Value::Primitive(crate::value::Primitive::Undefined));
                            let new_frame = crate::vm::Frame {
                                registers: regs,
                                stack: Vec::new(),
                                ip: 0,
                                function: c.function.clone(),
                                return_value: None,
                                curr_ret_reg: None,
                            };
                            proc.frames.push(new_frame);
                        }
                        _ => {
                            if let Some(frame) = proc.frames.last_mut() {
                                frame.registers[dst] =
                                    Value::Primitive(crate::value::Primitive::Undefined);
                            }
                        }
                    }
                    // let callee_frame = proc.frames.pop().unwrap();
                    // if let Some(frame) = proc.frames.last_mut() {
                    //     frame.registers[dst] = callee_frame
                    //         .return_value
                    //         .unwrap_or(Value::Primitive(crate::value::Primitive::Unit));
                    // }
                }
                Instruction::Return(reg) => {
                    // Set the return_value in the current frame, but do NOT pop the frame.
                    println!(
                        "Returning from function {reg} is set to {:?}",
                        proc.frames.last().unwrap().registers[reg]
                    );
                    if let Some(frame) = proc.frames.last_mut() {
                        let val = frame.registers[reg].clone();
                        frame.return_value = Some(val);
                    }
                    // Mark the process as needing to return (will be handled after the instruction)
                    // proc.alive = false;
                }
                Instruction::Jump(offset) => {
                    if let Some(frame) = proc.frames.last_mut() {
                        let new_ip = (frame.ip as isize + offset - 1) as usize;
                        frame.ip = new_ip;
                    }
                }
                Instruction::JumpIfTrue(cond_reg, offset) => {
                    let cond = if let Some(frame) = proc.frames.last() {
                        frame.registers[cond_reg].clone()
                    } else {
                        Value::Primitive(crate::value::Primitive::Undefined)
                    };
                    if let Value::Primitive(crate::value::Primitive::Boolean(true)) = cond {
                        if let Some(frame) = proc.frames.last_mut() {
                            let new_ip = (frame.ip as isize + offset - 1) as usize;
                            frame.ip = new_ip;
                        }
                    }
                }
                Instruction::JumpIfFalse(cond_reg, offset) => {
                    let cond = if let Some(frame) = proc.frames.last() {
                        frame.registers[cond_reg].clone()
                    } else {
                        Value::Primitive(crate::value::Primitive::Undefined)
                    };
                    if let Value::Primitive(crate::value::Primitive::Boolean(false)) = cond {
                        if let Some(frame) = proc.frames.last_mut() {
                            let new_ip = (frame.ip as isize + offset - 1) as usize;
                            frame.ip = new_ip;
                        }
                    }
                }
                Instruction::Spawn(dst, func_reg, arg_regs) => {
                    let (func_val, args) = if let Some(frame) = proc.frames.last() {
                        let func_val = &frame.registers[func_reg];
                        let args: Vec<Value> = arg_regs
                            .iter()
                            .map(|&i| frame.registers[i].clone())
                            .collect();
                        (func_val.clone(), args)
                    } else {
                        (Value::Primitive(crate::value::Primitive::Undefined), vec![])
                    };
                    if let Value::Function(f) = func_val {
                        let pid = self.spawn_process(f.clone(), args);
                        if let Some(frame) = proc.frames.last_mut() {
                            frame.registers[dst] =
                                Value::Process(self.processes.get(&pid).unwrap().clone());
                        }
                    } else {
                        if let Some(frame) = proc.frames.last_mut() {
                            frame.registers[dst] =
                                Value::Primitive(crate::value::Primitive::Undefined);
                        }
                    }
                }
                Instruction::Send(proc_reg, msg_reg) => {
                    let (proc_val, msg) = if let Some(frame) = proc.frames.last() {
                        (&frame.registers[proc_reg], frame.registers[msg_reg].clone())
                    } else {
                        (
                            &Value::Primitive(crate::value::Primitive::Undefined),
                            Value::Primitive(crate::value::Primitive::Undefined),
                        )
                    };
                    if let Value::Process(proc_rc) = proc_val {
                        proc_rc.borrow_mut().mailbox.push(msg);
                    }
                }
                Instruction::Receive(dst) => {
                    let msg = proc.mailbox.pop();
                    if let Some(msg) = msg {
                        if let Some(frame) = proc.frames.last_mut() {
                            frame.registers[dst] = msg;
                        }
                    } else {
                        self.run_queue.push_back(proc.pid);
                        continue;
                    }
                }
                Instruction::Link(proc_reg) => {
                    let (other_pid, need_link) = {
                        let proc_val = if let Some(frame) = proc.frames.last() {
                            &frame.registers[proc_reg]
                        } else {
                            return;
                        };
                        if let Value::Process(proc_rc) = proc_val {
                            let other_pid = proc_rc.borrow().pid;
                            let need_link = !proc.links.contains(&other_pid);
                            (other_pid, need_link)
                        } else {
                            return;
                        }
                    };
                    if need_link {
                        proc.links.push(other_pid);
                    }
                    let proc_val = if let Some(frame) = proc.frames.last() {
                        &frame.registers[proc_reg]
                    } else {
                        return;
                    };
                    if let Value::Process(proc_rc) = proc_val {
                        let mut other = proc_rc.borrow_mut();
                        if !other.links.contains(&proc.pid) {
                            other.links.push(proc.pid);
                        }
                    }
                }
                Instruction::Match(src_reg, pattern_table) => {
                    let val = if let Some(frame) = proc.frames.last() {
                        &frame.registers[src_reg]
                    } else {
                        &Value::Primitive(crate::value::Primitive::Undefined)
                    };
                    let mut matched = false;
                    for (pattern, offset) in pattern_table {
                        if pattern_matches(val, &pattern.clone()) {
                            if let Some(frame) = proc.frames.last_mut() {
                                let new_ip = (frame.ip as isize + offset - 1) as usize;
                                frame.ip = new_ip;
                            }
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        // No match, do nothing or set to Undefined
                    }
                }
                Instruction::Nop => {}
            }

            // After executing the instruction, check if the top frame has return_value set (i.e., function returned)
            // If so, pop the frame and copy the return_value to the caller's register as specified by the last Call
            loop {
                let ret_val = {
                    if let Some(frame) = proc.frames.last() {
                        frame.return_value.clone()
                    } else {
                        None
                    }
                };
                if let Some(val) = ret_val {
                    let _ = proc.frames.pop();
                    if let Some(caller_frame) = proc.frames.last_mut() {
                        caller_frame.registers[caller_frame.curr_ret_reg.unwrap()] = val;
                        // } else {
                        //     proc.last_result = Some(val);
                        //     proc.alive = false;
                        //     break;
                    }
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Function, Object, Primitive, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn dummy_function() -> Rc<Function> {
        Rc::new(Function {
            name: Some("dummy".to_string()),
            bytecode: vec![],
            arity: 0,
        })
    }

    #[test]
    fn test_spawn_process() {
        let mut vm = VM::new();
        let func = dummy_function();
        let pid = vm.spawn_process(func.clone(), vec![Value::Primitive(Primitive::Number(1.0))]);
        assert_eq!(pid, 1);
        assert!(vm.processes.contains_key(&pid));
        assert_eq!(vm.run_queue.len(), 1);
    }

    #[test]
    fn test_schedule_round_robin() {
        let mut vm = VM::new();
        let func = dummy_function();
        let pid1 = vm.spawn_process(func.clone(), vec![]);
        let pid2 = vm.spawn_process(func.clone(), vec![]);
        let pid3 = vm.spawn_process(func.clone(), vec![]);
        let mut scheduled = vec![];
        for _ in 0..3 {
            let proc = vm.schedule().unwrap();
            scheduled.push(proc.borrow().pid);
        }
        scheduled.sort();
        assert_eq!(scheduled, vec![1, 2, 3]);
    }

    #[test]
    fn test_schedule_skips_dead_process() {
        let mut vm = VM::new();
        let func = dummy_function();
        let pid1 = vm.spawn_process(func.clone(), vec![]);
        let pid2 = vm.spawn_process(func.clone(), vec![]);
        // Kill process 1
        if let Some(proc1) = vm.processes.get(&pid1) {
            proc1.borrow_mut().alive = false;
        }
        // Should only schedule process 2
        let proc = vm.schedule().unwrap();
        assert_eq!(proc.borrow().pid, pid2);
    }

    #[test]
    fn test_dispatch_add_and_return() {
        use crate::vm::Instruction;
        // Bytecode: r0 = 2.0; r1 = 3.0; r2 = r0 + r1; return r2
        let bytecode = vec![
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(2.0))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(3.0))),
            Instruction::Add(2, 0, 1),
            Instruction::Return(2),
        ];
        let func = Rc::new(Function {
            name: Some("add".to_string()),
            bytecode,
            arity: 0,
        });
        let mut vm = VM::new();
        let pid = vm.spawn_process(func.clone(), vec![]);
        vm.run();
        let proc = vm.processes.get(&pid).unwrap();
        assert!(!proc.borrow().alive);
    }

    #[test]
    fn test_property_access() {
        use crate::vm::Instruction;
        // Create an object with property "foo" = 42
        let mut obj = Object::new(None);
        obj.set_property("foo", Value::Primitive(Primitive::Number(42.0)));
        let obj_rc = Rc::new(RefCell::new(obj));
        // Bytecode: r0 = obj, r1 = "foo", r2 = obj["foo"]
        let bytecode = vec![
            Instruction::LoadConst(0, Value::Object(obj_rc.clone())),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("foo".to_string()))),
            Instruction::GetProp(2, 0, 1),
            Instruction::Return(2),
        ];
        let func = Rc::new(Function {
            name: Some("getprop".to_string()),
            bytecode,
            arity: 0,
        });
        let mut vm = VM::new();
        let pid = vm.spawn_process(func.clone(), vec![]);
        vm.run();
        let proc = vm.processes.get(&pid).unwrap();
        // The result should be 42 in last_result after process completion
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(42.0)))
        );
    }

    #[test]
    fn test_function_call() {
        use crate::vm::Instruction;
        // Inner function: r0 = 10, r1 = 20, r2 = r0 + r1, return r2
        let inner_bytecode = vec![
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(10.0))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(20.0))),
            Instruction::Add(2, 0, 1),
            Instruction::Return(2),
        ];
        let inner_func = Rc::new(Function {
            name: Some("add".to_string()),
            bytecode: inner_bytecode,
            arity: 0,
        });
        // Outer function: r0 = inner_func, call r0, return r0
        let outer_bytecode = vec![
            Instruction::LoadConst(0, Value::Function(inner_func.clone())),
            Instruction::Call(1, 0, vec![]),
            Instruction::Return(1),
        ];
        let outer_func = Rc::new(Function {
            name: Some("outer".to_string()),
            bytecode: outer_bytecode,
            arity: 0,
        });
        let mut vm = VM::new();
        let pid = vm.spawn_process(outer_func.clone(), vec![]);
        vm.run();
        let proc = vm.processes.get(&pid).unwrap();
        // The result should be 30 in last_result after process completion
        assert_eq!(
            proc.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(30.0)))
        );
    }

    #[test]
    fn test_concurrency_send_receive() {
        use crate::vm::Instruction;
        // Process 1: send 42 to process 2, return unit
        let proc2_placeholder = Value::Primitive(Primitive::Undefined); // Will be replaced
        let proc1_bytecode = vec![
            Instruction::LoadConst(0, proc2_placeholder.clone()),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(42.0))),
            Instruction::Send(0, 1),
            Instruction::Return(0),
        ];
        let proc1_func = Rc::new(Function {
            name: Some("sender".to_string()),
            bytecode: proc1_bytecode,
            arity: 0,
        });
        // Process 2: receive into r0, return r0
        let proc2_bytecode = vec![Instruction::Receive(0), Instruction::Return(0)];
        let proc2_func = Rc::new(Function {
            name: Some("receiver".to_string()),
            bytecode: proc2_bytecode,
            arity: 0,
        });
        let mut vm = VM::new();
        let pid2 = vm.spawn_process(proc2_func.clone(), vec![]);
        let pid1 = vm.spawn_process(proc1_func.clone(), vec![]);
        // Patch proc1's bytecode to have the correct process handle for pid2
        if let Some(proc1) = vm.processes.get(&pid1) {
            if let Some(frame) = proc1.borrow_mut().frames.last_mut() {
                frame.registers[0] = Value::Process(vm.processes.get(&pid2).unwrap().clone());
            }
        }
        vm.run();
        let proc2 = vm.processes.get(&pid2).unwrap();
        // The result should be 42 in last_result after process completion
        assert_eq!(
            proc2.borrow().last_result,
            Some(Value::Primitive(Primitive::Number(42.0)))
        );
    }
}
