use super::IonVM;
use crate::value::process::ProcessStatus;
use std::time::{SystemTime, UNIX_EPOCH};

// Tracks a pending ReceiveWithTimeout for a blocked process.
pub(crate) struct TimeoutInfo {
    pub pid: usize,
    pub dst_reg: usize,
    pub result_reg: usize,
    pub expiry_ms: u64,
}

impl IonVM {
    pub fn handle_expired_timeouts(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let (expired, remaining): (Vec<_>, Vec<_>) = self
            .pending_timeouts
            .drain(..)
            .partition(|t| now >= t.expiry_ms);
        self.pending_timeouts = remaining;

        for t in expired {
            if let Some(proc_rc) = self.processes.get(&t.pid) {
                let mut proc = proc_rc.borrow_mut();
                if proc.status == ProcessStatus::WaitingForMessage {
                    // Default values (false, Undefined) were pre-set when the timeout was registered,
                    // so there is nothing more to write to registers here.
                    proc.status = ProcessStatus::Runnable;
                    drop(proc);
                    if !self.run_queue.contains(&t.pid) {
                        self.run_queue.push_back(t.pid);
                    }
                    if self.debug {
                        eprintln!("[VM] timeout fired for process {}", t.pid);
                    }
                }
            }
        }
    }

    // Remove and return the pending timeout registers for a process, if one exists.
    pub(crate) fn take_pending_timeout(&mut self, pid: usize) -> Option<(usize, usize)> {
        let idx = self.pending_timeouts.iter().position(|t| t.pid == pid)?;
        let t = self.pending_timeouts.remove(idx);
        Some((t.dst_reg, t.result_reg))
    }
}
