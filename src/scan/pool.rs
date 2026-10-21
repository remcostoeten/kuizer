use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const SHARE_ABOVE: usize = 64;
const IDLE_WAIT: Duration = Duration::from_millis(20);

/// Work shared by a pool of threads. Each thread keeps its own stack and hands half of it to
/// the shared list when another thread runs dry, so one deep or wide subtree never pins a
/// single thread while the rest sit idle.
pub(crate) struct Queue<T> {
    shared: Mutex<Vec<T>>,
    ready: Condvar,
    shared_len: AtomicUsize,
    outstanding: AtomicUsize,
    idle: AtomicUsize,
    stopped: AtomicBool,
}

impl<T> Queue<T> {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _guard = self.shared.lock();
        self.ready.notify_all();
    }

    pub fn new(seed: Vec<T>) -> Self {
        Self {
            outstanding: AtomicUsize::new(seed.len()),
            shared_len: AtomicUsize::new(seed.len()),
            shared: Mutex::new(seed),
            ready: Condvar::new(),
            idle: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
        }
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    pub fn local(&self) -> Local<'_, T> {
        Local {
            queue: self,
            stack: Vec::new(),
        }
    }
}

pub(crate) struct Local<'q, T> {
    queue: &'q Queue<T>,
    stack: Vec<T>,
}

impl<T> Local<'_, T> {
    pub fn push(&mut self, task: T) {
        self.queue.outstanding.fetch_add(1, Ordering::Relaxed);
        self.stack.push(task);
    }

    /// @name Local::next
    /// @description Returns the next task for this thread: its own newest, else a batch taken
    /// from the shared list, waiting while other threads may still produce work. `None` once
    /// every task is done or the queue was stopped.
    ///
    /// @example
    /// while let Some(task) = local.next() { run(task, &mut local); local.done(); }
    pub fn next(&mut self) -> Option<T> {
        if self.queue.stopped() {
            return None;
        }
        self.share();
        if let Some(task) = self.stack.pop() {
            return Some(task);
        }
        let queue = self.queue;
        let mut shared = queue
            .shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        loop {
            if queue.stopped() {
                return None;
            }
            if !shared.is_empty() {
                let take = shared.len().div_ceil(2);
                let from = shared.len() - take;
                self.stack.extend(shared.drain(from..));
                queue.shared_len.store(shared.len(), Ordering::Relaxed);
                return self.stack.pop();
            }
            if queue.outstanding.load(Ordering::Acquire) == 0 {
                return None;
            }
            queue.idle.fetch_add(1, Ordering::Relaxed);
            shared = queue
                .ready
                .wait_timeout(shared, IDLE_WAIT)
                .unwrap_or_else(|poison| poison.into_inner())
                .0;
            queue.idle.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Marks the task returned by the last `next` as finished.
    pub fn done(&mut self) {
        if self.queue.outstanding.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _guard = self.queue.shared.lock();
            self.queue.ready.notify_all();
        }
    }

    fn share(&mut self) {
        if self.stack.len() < 2 {
            return;
        }
        let wanted = self.queue.idle.load(Ordering::Relaxed) > 0
            || (self.stack.len() > SHARE_ABOVE
                && self.queue.shared_len.load(Ordering::Relaxed) == 0);
        if !wanted {
            return;
        }
        let top = self.stack.split_off(self.stack.len() / 2);
        let bottom = std::mem::replace(&mut self.stack, top);
        let mut shared = self
            .queue
            .shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        shared.extend(bottom);
        self.queue.shared_len.store(shared.len(), Ordering::Relaxed);
        self.queue.ready.notify_all();
    }
}
