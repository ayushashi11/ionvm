pub mod frame;
pub mod process;
pub mod optimizer;
pub mod executor;
pub mod simd;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Condvar};
use crate::bytecode::instruction::{Bytecode, OperandType};
use crate::lang::types::Value;
use crate::vm::frame::Frame;
use crate::vm::process::{Process, ProcessState, WaitReason, WaitState};
pub use executor::ExecutionResult;

pub const PROC_REDUCTIONS: usize = 2_000;

pub struct SchedulerState {
    pub ready_queue: VecDeque<Process>,
    pub waiting_processes: HashMap<usize, (Process, WaitState)>,
    pub running_mailboxes: HashMap<usize, VecDeque<Value>>,
    pub process_monitors: HashMap<usize, Vec<usize>>,
    pub next_wait_id: u64,
    pub active_processes: usize,
    pub next_pid: usize,
    pub last_error: Option<String>,
    pub entrypoint_return: Option<Value>,
}


pub struct Scheduler {
    pub state: Mutex<SchedulerState>,
    pub cond: Condvar,
    pub bytecode: Arc<Bytecode>,
    pub function_indices: Arc<HashMap<String, usize>>,
}

impl Scheduler {
    pub fn spawn_process(&self, func_name: &str, args: Vec<Value>) -> Result<usize, String> {
        let fn_idx = self
            .function_indices
            .get(func_name)
            .copied()
            .ok_or_else(|| format!("function '{}' not found", func_name))?;

        let mut state = self.state.lock().unwrap();
        let pid = state.next_pid;
        state.next_pid += 1;

        let mut proc = Process::new(pid, PROC_REDUCTIONS);
        let func = &self.bytecode.functions[fn_idx];
        let frame = Frame::new(fn_idx, func.register_count, args, OperandType::Register(0));
        proc.frame_stack.push(frame);

        state.active_processes += 1;
        state.ready_queue.push_back(proc);
        self.cond.notify_one();
        Ok(pid)
    }

    pub fn send_message(&self, target_pid: usize, msg: Value) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        // 1. Target is waiting for a message: check reason and wake
        if let Some((mut target, wait_state)) = state.waiting_processes.remove(&target_pid) {
            let matched = match &wait_state.reason {
                WaitReason::AnyMessage => true,
                WaitReason::SelectiveMessage { tag, .. } => {
                    tag.as_ref().map_or(true, |expected_tag| {
                        if let Value::Object(obj) = &msg {
                            if let Ok(o) = obj.read() {
                                if let Some(Value::String(actual_tag)) = o.get_property("@@") {
                                    return actual_tag == *expected_tag;
                                }
                            }
                        }
                        false
                    })
                }
                _ => false,
            };

            if matched {
                target.mailbox.push_back(msg);
                target.state = ProcessState::Running;
                state.ready_queue.push_back(target);
                self.cond.notify_one();
                return Ok(());
            } else {
                target.mailbox.push_back(msg);
                state.waiting_processes.insert(target_pid, (target, wait_state));
                return Ok(());
            }
        }
        // 2. Target is in ready_queue
        for p in state.ready_queue.iter_mut() {
            if p.id == target_pid {
                p.mailbox.push_back(msg);
                return Ok(());
            }
        }
        // 3. Target is currently executing on another worker thread
        state.running_mailboxes.entry(target_pid).or_default().push_back(msg);
        Ok(())
    }

    pub fn wake_timeout(&self, pid: usize, wait_id: u64) {
        let mut state = self.state.lock().unwrap();
        if let Some((_, wait_state)) = state.waiting_processes.get(&pid) {
            if wait_state.wait_id != wait_id {
                return;
            }
        } else {
            return;
        }

        let (mut process, wait_state) = state.waiting_processes.remove(&pid).unwrap();
        if let Some(dst_reg) = wait_state.dst_reg {
            let val = wait_state.timeout_val.unwrap_or(Value::Nil);
            if let Some(frame) = process.frame_stack.last_mut() {
                if dst_reg < frame.registers.len() {
                    frame.registers[dst_reg] = val;
                }
                frame.pc += 1;
            }
        }
        process.state = ProcessState::Running;
        state.ready_queue.push_back(process);
        self.cond.notify_one();
    }
}

pub fn worker_loop(sched: Arc<Scheduler>) {
    loop {
        let mut state = sched.state.lock().unwrap();
        while state.ready_queue.is_empty() {
            if state.active_processes == 0 {
                sched.cond.notify_all();
                return;
            }
            state = sched.cond.wait(state).unwrap();
            if state.active_processes == 0 && state.ready_queue.is_empty() {
                sched.cond.notify_all();
                return;
            }
        }

        let mut process = state.ready_queue.pop_front().unwrap();
        if let Some(pending) = state.running_mailboxes.remove(&process.id) {
            process.mailbox.extend(pending);
        }
        drop(state);

        process.reductions = PROC_REDUCTIONS;
        let mut ctx = executor::ExecutionContext {
            bytecode: &sched.bytecode,
            process: &mut process,
            scheduler: &sched,
        };

        let result = executor::fetch_and_dispatch(&mut ctx);

        let mut state = sched.state.lock().unwrap();
        if let Some(pending) = state.running_mailboxes.remove(&process.id) {
            process.mailbox.extend(pending);
        }

        match result {
            ExecutionResult::Continue => {
                process.state = ProcessState::Running;
                state.ready_queue.push_back(process);
                sched.cond.notify_one();
            }
            ExecutionResult::Wait(wait_state) => {
                if !process.mailbox.is_empty() {
                    process.state = ProcessState::Running;
                    state.ready_queue.push_back(process);
                    sched.cond.notify_one();
                } else {
                    let wait_id = state.next_wait_id;
                    state.next_wait_id += 1;
                    let pid = process.id;
                    let timeout = wait_state.timeout;

                    let mut ws = *wait_state;
                    ws.wait_id = wait_id;
                    process.state = ProcessState::Waiting(ws.clone());
                    state.waiting_processes.insert(pid, (process, ws));

                    if let Some(duration) = timeout {
                        let sched_clone = Arc::clone(&sched);
                        std::thread::spawn(move || {
                            std::thread::sleep(duration);
                            sched_clone.wake_timeout(pid, wait_id);
                        });
                    }
                }
            }
            ExecutionResult::Terminate => {
                if process.id == 0 {
                    state.entrypoint_return = process.entrypoint_return.clone();
                }
                process.state = ProcessState::Terminated;
                state.active_processes = state.active_processes.saturating_sub(1);
                if let Some(observers) = state.process_monitors.remove(&process.id) {
                    for obs_pid in observers {
                        if let Some((mut obs_proc, obs_wait)) = state.waiting_processes.remove(&obs_pid) {
                            if let Some(dst_reg) = obs_wait.dst_reg {
                                let exit_val = process.entrypoint_return.clone().unwrap_or(Value::Nil);
                                if let Some(frame) = obs_proc.frame_stack.last_mut() {
                                    if dst_reg < frame.registers.len() {
                                        frame.registers[dst_reg] = exit_val;
                                    }
                                    frame.pc += 1;
                                }
                            }
                            obs_proc.state = ProcessState::Running;
                            state.ready_queue.push_back(obs_proc);
                            sched.cond.notify_one();
                        }
                    }
                }
                if state.active_processes == 0 {
                    sched.cond.notify_all();
                }
            }
            ExecutionResult::Error(msg) => {
                eprintln!("[Process {} error]: {}", process.id, msg);
                if state.last_error.is_none() {
                    state.last_error = Some(format!("Process {} error: {}", process.id, msg));
                }
                process.state = ProcessState::Terminated;
                state.active_processes = state.active_processes.saturating_sub(1);
                if state.active_processes == 0 {
                    sched.cond.notify_all();
                }
            }
        }
    }
}

pub struct IonVM {
    pub scheduler: Arc<Scheduler>,
    pub enable_cuda: bool,
}

impl IonVM {
    pub fn new(bytecode: Bytecode) -> Result<Self, String> {
        Self::new_with_options(bytecode, false)
    }

    pub fn new_with_options(mut bytecode: Bytecode, enable_cuda: bool) -> Result<Self, String> {
        // Load external native dynamic libraries declared in bytecode metadata
        for decl in &bytecode.native_declarations {
            crate::builtin::native_lib::load_native_library(&decl.lib_name, &decl.namespace)
                .map_err(|e| format!("Native library error: {}", e))?;
        }

        let mut function_indices = HashMap::new();
        let mut main_idx = None;

        optimizer::optimize_bytecode(&mut bytecode);

        for (i, func) in bytecode.functions.iter_mut().enumerate() {
            if func.name != "main" {
                if let Some(native_fn) = crate::jit::translator::compile_function(func) {
                    func.jit_compiled_fn = Some(native_fn);
                }
            }
            function_indices.insert(func.name.clone(), i);
            if func.name == "main" {
                main_idx = Some(i);
            }
        }

        let mut jit_indices: HashMap<String, usize> = HashMap::new();
        for (i, func) in bytecode.functions.iter().enumerate() {
            if func.jit_compiled_fn.is_some() {
                jit_indices.insert(func.name.clone(), i);
            }
        }

        for func in bytecode.functions.iter_mut() {
            for inst in func.instructions.iter_mut() {
                if inst.opcode == crate::bytecode::opcode::Opcode::Call && !inst.operands.is_empty() {
                    if let OperandType::Function(ref name) = inst.operands[0] {
                        if let Some(&idx) = jit_indices.get(name) {
                            inst.opcode = crate::bytecode::opcode::Opcode::CallJit;
                            inst.operands[0] = OperandType::Offset(idx as i64);
                        }
                    }
                }
            }
        }

        let main_fn_index = main_idx.ok_or_else(|| "main function not found in bytecode".to_string())?;

        let mut ready_queue = VecDeque::new();
        let main_proc_id = 0;
        let mut main_proc = Process::new(main_proc_id, PROC_REDUCTIONS);

        let main_func = &bytecode.functions[main_fn_index];
        let initial_frame = Frame::new(
            main_fn_index,
            main_func.register_count,
            Vec::new(),
            OperandType::Register(0),
        );
        main_proc.frame_stack.push(initial_frame);
        ready_queue.push_back(main_proc);

        let state = SchedulerState {
            ready_queue,
            waiting_processes: HashMap::new(),
            running_mailboxes: HashMap::new(),
            process_monitors: HashMap::new(),
            next_wait_id: 1,
            active_processes: 1,
            next_pid: 1,
            last_error: None,
            entrypoint_return: None,
        };


        let scheduler = Arc::new(Scheduler {
            state: Mutex::new(state),
            cond: Condvar::new(),
            bytecode: Arc::new(bytecode),
            function_indices: Arc::new(function_indices),
        });

        Ok(IonVM { scheduler, enable_cuda })
    }

    pub fn spawn_process(&self, func_name: &str, args: Vec<Value>) -> Result<usize, String> {
        self.scheduler.spawn_process(func_name, args)
    }

    pub fn send_message(&self, target_pid: usize, msg: Value) -> Result<(), String> {
        self.scheduler.send_message(target_pid, msg)
    }

    pub fn step(&mut self) -> Result<bool, String> {
        let mut state = self.scheduler.state.lock().unwrap();
        if state.ready_queue.is_empty() {
            return Ok(false);
        }
        let mut process = state.ready_queue.pop_front().unwrap();
        if let Some(pending) = state.running_mailboxes.remove(&process.id) {
            process.mailbox.extend(pending);
        }
        drop(state);

        process.reductions = PROC_REDUCTIONS;
        let mut ctx = executor::ExecutionContext {
            bytecode: &self.scheduler.bytecode,
            process: &mut process,
            scheduler: &self.scheduler,
        };
        let result = executor::fetch_and_dispatch(&mut ctx);

        let mut state = self.scheduler.state.lock().unwrap();
        if let Some(pending) = state.running_mailboxes.remove(&process.id) {
            process.mailbox.extend(pending);
        }

        match result {
            ExecutionResult::Continue => {
                process.state = ProcessState::Running;
                state.ready_queue.push_back(process);
            }
            ExecutionResult::Wait(wait_state) => {
                if !process.mailbox.is_empty() {
                    process.state = ProcessState::Running;
                    state.ready_queue.push_back(process);
                } else {
                    let wait_id = state.next_wait_id;
                    state.next_wait_id += 1;
                    let pid = process.id;
                    let timeout = wait_state.timeout;

                    let mut ws = *wait_state;
                    ws.wait_id = wait_id;
                    process.state = ProcessState::Waiting(ws.clone());
                    state.waiting_processes.insert(pid, (process, ws));

                    if let Some(duration) = timeout {
                        let sched_clone = Arc::clone(&self.scheduler);
                        std::thread::spawn(move || {
                            std::thread::sleep(duration);
                            sched_clone.wake_timeout(pid, wait_id);
                        });
                    }
                }
            }
            ExecutionResult::Terminate => {
                process.state = ProcessState::Terminated;
                state.active_processes = state.active_processes.saturating_sub(1);
                if let Some(observers) = state.process_monitors.remove(&process.id) {
                    for obs_pid in observers {
                        if let Some((mut obs_proc, obs_wait)) = state.waiting_processes.remove(&obs_pid) {
                            if let Some(dst_reg) = obs_wait.dst_reg {
                                let exit_val = process.entrypoint_return.clone().unwrap_or(Value::Nil);
                                if let Some(frame) = obs_proc.frame_stack.last_mut() {
                                    if dst_reg < frame.registers.len() {
                                        frame.registers[dst_reg] = exit_val;
                                    }
                                    frame.pc += 1;
                                }
                            }
                            obs_proc.state = ProcessState::Running;
                            state.ready_queue.push_back(obs_proc);
                        }
                    }
                }
            }
            ExecutionResult::Error(msg) => {
                process.state = ProcessState::Terminated;
                state.active_processes = state.active_processes.saturating_sub(1);
                return Err(format!("Process {} error: {}", process.id, msg));
            }
        }
        Ok(true)
    }

    pub fn run(&mut self) -> Result<(), String> {
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let thread_count = cpu_count.max(2);

        let mut handles = Vec::with_capacity(thread_count);
        for _ in 0..thread_count {
            let sched = Arc::clone(&self.scheduler);
            handles.push(std::thread::spawn(move || {
                worker_loop(sched);
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        let state = self.scheduler.state.lock().unwrap();
        if let Some(err) = &state.last_error {
            return Err(err.clone());
        }

        Ok(())
    }

    pub fn entrypoint_return(&self) -> Option<Value> {
        self.scheduler.state.lock().unwrap().entrypoint_return.clone()
    }
}
