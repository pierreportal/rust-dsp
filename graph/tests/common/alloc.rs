//! A counting global allocator, so "the audio thread does not allocate" is a
//! measured fact rather than an assumption.
//!
//! This exists because the guarantee decays silently. An allocation on the audio
//! thread surfaces as an occasional dropout under load, weeks later, with no
//! obvious cause and no stack trace pointing anywhere useful. A test that fails
//! the moment somebody reintroduces a `clone` or a `vec![]` in the render path
//! costs nothing to keep.
//!
//! The counter is thread-local and armed explicitly, so tests running in parallel
//! do not see each other's traffic and setup outside the measured region is free.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// Wraps the system allocator and counts allocations while armed.
pub struct Counting;

// SAFETY: every method forwards to `System` unchanged. The only added behaviour
// is bumping a thread-local counter, which does not affect the returned memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: `layout` is forwarded to the system allocator verbatim.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` came from `System` and are freed as such.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        // SAFETY: `ptr` and `layout` came from `System` and are resized as such.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn record() {
    // `try_with` because the allocator can be reached during thread-local
    // teardown, when the TLS destructors have already run. Panicking there would
    // abort the process for no benefit, so the count is simply skipped.
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

/// Count allocations on this thread until the returned guard is dropped.
pub fn counting() -> Guard {
    ARMED.with(|a| a.set(true));
    ALLOCATIONS.with(|n| n.set(0));
    Guard
}

pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = ARMED.try_with(|a| a.set(false));
    }
}

/// Allocations counted on this thread since [`counting`] was called.
pub fn allocations() -> usize {
    ALLOCATIONS.try_with(|n| n.get()).unwrap_or(0)
}
