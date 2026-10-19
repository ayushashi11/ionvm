use std::collections::VecDeque;
use crate::lang::types::Value;
use crate::vm::frame::Frame;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessState {
    Running,
    WaitingMessage,
    WaitingIo,
    Terminated,
}

#[derive(Clone, Debug)]
pub struct Process {
    pub id: usize,
    pub mailbox: VecDeque<Value>,
    pub frame_stack: Vec<Frame>,
    pub state: ProcessState,
    pub reductions: usize,
    pub entrypoint_return: Option<Value>,
}

impl Process {
    pub fn new(id: usize, reductions: usize) -> Self {
        Process {
            id,
            mailbox: VecDeque::new(),
            frame_stack: Vec::new(),
            state: ProcessState::Running,
            reductions,
            entrypoint_return: None,
        }
    }
}
