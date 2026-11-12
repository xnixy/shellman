//! Every process in one call: `NtQuerySystemInformation(SystemProcessInformation)` gives each
//! process's times, memory, I/O counts, handle and thread counts and its threads' states, which
//! is all the list needs. And each core's times (`SystemProcessorPerformanceInformation`).
//!
//! The structures are the documented-by-practice x64 layouts (the same Process Hacker and the
//! Windows SDK's winternl.h describe); their sizes are checked in the tests.

use super::cpu::Times;
use std::ffi::c_void;

#[link(name = "ntdll", kind = "raw-dylib")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
    pub(super) fn NtQueryInformationProcess(process: *mut c_void, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

const SYSTEM_PROCESS_INFORMATION: u32 = 5;
const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION: u32 = 8;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023_u32 as i32;
/// A thread waiting (state 5), because it is suspended (wait reason 5).
const THREAD_WAITING: u32 = 5;
const WAIT_SUSPENDED: u32 = 5;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[repr(C)]
struct ProcessInfo {
    next_entry_offset: u32,
    number_of_threads: u32,
    working_set_private_size: i64,
    hard_fault_count: u32,
    number_of_threads_high_watermark: u32,
    cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    image_name: UnicodeString,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
    handle_count: u32,
    session_id: u32,
    unique_process_key: usize,
    peak_virtual_size: usize,
    virtual_size: usize,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_page_count: usize,
    read_operation_count: i64,
    write_operation_count: i64,
    other_operation_count: i64,
    read_transfer_count: i64,
    write_transfer_count: i64,
    other_transfer_count: i64,
}

#[repr(C)]
struct ThreadInfo {
    kernel_time: i64,
    user_time: i64,
    create_time: i64,
    wait_time: u32,
    start_address: usize,
    client_id: [usize; 2],
    priority: i32,
    base_priority: i32,
    context_switches: u32,
    thread_state: u32,
    wait_reason: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessorTimes {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    interrupt_count: u32,
}

/// One process as the system list has it.
#[derive(Clone, Debug, Default)]
pub struct Raw {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub created: i64,
    /// Kernel and user time, 100 ns steps.
    pub cpu_time: i64,
    pub private_ws: u64,
    pub threads: u32,
    pub handles: u32,
    pub session: u32,
    /// Bytes read and written so far.
    pub io_bytes: u64,
    pub suspended: bool,
}

/// Reads the list into a buffer it keeps, so a tick allocates nothing once it has grown.
#[derive(Default)]
pub struct Reader {
    buf: Vec<u64>,
}

impl Reader {
    fn query(&mut self, class: u32) -> Option<usize> {
        if self.buf.is_empty() {
            self.buf = vec![0; 64 * 1024];
        }
        for _ in 0..8 {
            let bytes = (self.buf.len() * 8) as u32;
            let mut needed = 0u32;
            let status = unsafe { NtQuerySystemInformation(class, self.buf.as_mut_ptr().cast(), bytes, &mut needed) };
            match status {
                s if s >= 0 => return Some(needed.min(bytes) as usize),
                STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL => {
                    // Room for what it asked plus processes started meanwhile.
                    let want = (needed as usize).max(bytes as usize * 2) + 64 * 1024;
                    self.buf = vec![0; want / 8 + 1];
                }
                _ => return None,
            }
        }
        None
    }

    /// Every process now (the idle process, PID 0, left out).
    pub fn processes(&mut self) -> Vec<Raw> {
        let mut out = Vec::with_capacity(400);
        let Some(len) = self.query(SYSTEM_PROCESS_INFORMATION) else {
            return out;
        };
        let base = self.buf.as_ptr() as *const u8;
        let mut offset = 0usize;
        loop {
            if offset + size_of::<ProcessInfo>() > len {
                break;
            }
            // SAFETY: the entry lies inside the buffer the call filled, and entries are aligned.
            let p = unsafe { &*(base.add(offset) as *const ProcessInfo) };
            if p.unique_process_id != 0 {
                out.push(raw(p));
            }
            if p.next_entry_offset == 0 {
                break;
            }
            offset += p.next_entry_offset as usize;
        }
        out
    }

    /// Each core's idle, kernel and user times.
    /// Each core's idle, kernel and user times (the cores of this process's processor group).
    pub fn cores(&mut self, count: usize) -> Vec<Times> {
        // This class wants a buffer of whole entries, at most one per core.
        let mut times = vec![ProcessorTimes::default(); count.clamp(1, 64)];
        let bytes = (times.len() * size_of::<ProcessorTimes>()) as u32;
        let mut len = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION, times.as_mut_ptr().cast(), bytes, &mut len)
        };
        if status < 0 {
            return Vec::new();
        }
        times.truncate(len as usize / size_of::<ProcessorTimes>());
        times.iter().map(|t| Times { idle: t.idle, kernel: t.kernel, user: t.user }).collect()
    }
}

fn raw(p: &ProcessInfo) -> Raw {
    let name = if p.image_name.buffer.is_null() || p.image_name.length == 0 {
        if p.unique_process_id == 4 { "System".to_owned() } else { String::new() }
    } else {
        // SAFETY: the name points into the same buffer, `length` bytes long.
        let s = unsafe { std::slice::from_raw_parts(p.image_name.buffer, p.image_name.length as usize / 2) };
        String::from_utf16_lossy(s)
    };
    let threads = p.number_of_threads as usize;
    // SAFETY: the thread entries follow the process entry, `number_of_threads` of them.
    let thread_list = unsafe {
        let first = (p as *const ProcessInfo).add(1) as *const ThreadInfo;
        std::slice::from_raw_parts(first, threads)
    };
    let suspended =
        threads > 0 && thread_list.iter().all(|t| t.thread_state == THREAD_WAITING && t.wait_reason == WAIT_SUSPENDED);
    Raw {
        pid: p.unique_process_id as u32,
        parent: p.inherited_from_unique_process_id as u32,
        name,
        created: p.create_time,
        cpu_time: p.kernel_time + p.user_time,
        private_ws: p.working_set_private_size.max(0) as u64,
        threads: p.number_of_threads,
        handles: p.handle_count,
        session: p.session_id,
        io_bytes: (p.read_transfer_count.max(0) + p.write_transfer_count.max(0)) as u64,
        suspended,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_windows() {
        assert_eq!(size_of::<ProcessInfo>(), 0x100);
        assert_eq!(size_of::<ThreadInfo>(), 0x50);
        assert_eq!(size_of::<ProcessorTimes>(), 48);
        assert_eq!(std::mem::offset_of!(ProcessInfo, image_name), 0x38);
        assert_eq!(std::mem::offset_of!(ProcessInfo, unique_process_id), 0x50);
        assert_eq!(std::mem::offset_of!(ProcessInfo, read_transfer_count), 0xE8);
    }

    #[test]
    fn this_process_is_in_the_list() {
        let mut r = Reader::default();
        let all = r.processes();
        let me = all.iter().find(|p| p.pid == std::process::id()).expect("our own process");
        assert!(me.threads >= 1 && me.handles > 0 && me.created > 0);
        assert!(me.private_ws > 0);
        assert!(!me.suspended);
        assert!(all.iter().any(|p| p.pid == 4 && p.name == "System"));
        assert!(all.len() > 10);
    }

    #[test]
    fn cores_are_read() {
        let mut r = Reader::default();
        let c = r.cores(256);
        assert!(!c.is_empty());
        assert!(c[0].kernel >= c[0].idle);
    }
}
