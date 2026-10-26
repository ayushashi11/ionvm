use std::collections::VecDeque;
use std::time::Duration;
use crate::lang::types::Value;
use crate::vm::frame::Frame;

#[derive(Clone, Debug, PartialEq)]
pub enum WaitReason {
    AnyMessage,
    SelectiveMessage {
        tag: Option<String>,
        from_pid: Option<usize>,
    },
    ProcessExit {
        target_pid: usize,
    },
    NativeEvent {
        event_id: u64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaitState {
    pub wait_id: u64,
    pub reason: WaitReason,
    pub timeout: Option<Duration>,
    pub dst_reg: Option<usize>,
    pub timeout_val: Option<Value>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ProcessState {
    Ready,
    Running,
    Waiting(WaitState),
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
