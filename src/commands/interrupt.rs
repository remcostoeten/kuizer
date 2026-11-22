use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_interrupt(_signal: libc::c_int) {
    REQUESTED.store(true, Ordering::SeqCst);
    // SAFETY: signal is async-signal-safe; a second Ctrl-C then ends the process as usual.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

pub(super) struct Interrupt {
    previous: libc::sighandler_t,
}

impl Interrupt {
    pub(super) fn catch() -> Self {
        REQUESTED.store(false, Ordering::SeqCst);
        let handler = on_interrupt as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: the handler only touches an atomic and calls signal.
        let previous = unsafe { libc::signal(libc::SIGINT, handler) };
        Self { previous }
    }

    pub(super) fn requested(&self) -> bool {
        REQUESTED.load(Ordering::SeqCst)
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        // SAFETY: restores the handler that was installed before `catch`.
        unsafe {
            libc::signal(libc::SIGINT, self.previous);
        }
    }
}
