use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{Pid, Signal, kill_process_group};

pub(crate) const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const MUTATION_TIMEOUT: Duration = Duration::from_secs(300);
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(2);
const SPAWN_RETRY_DELAY: Duration = Duration::from_millis(20);

/// @name run
/// @description Runs a noninteractive command, captures both output streams and kills the
/// child process group if it does not finish in time.
///
/// @example
/// let output = run(Command::new("docker").arg("version"), QUERY_TIMEOUT)?;
pub(crate) fn run(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    let start = Instant::now();
    command
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = spawn(command, start, timeout)?;
    let result = capture(&mut child, start, timeout);
    if result.is_err() {
        let _ = kill_process_group(Pid::from_child(&child), Signal::KILL);
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// @name git
/// @description Runs Git in a repository with hooks, fsmonitor and optional locks disabled.
///
/// @example
/// let status = git(root, &["status", "--porcelain"], QUERY_TIMEOUT)?;
pub(crate) fn git(root: &Path, args: &[&str], timeout: Duration) -> io::Result<Output> {
    run(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0"),
        timeout,
    )
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "command did not finish in time")
}

fn remaining(start: Instant, timeout: Duration) -> io::Result<Duration> {
    timeout
        .checked_sub(start.elapsed())
        .filter(|left| !left.is_zero())
        .ok_or_else(timed_out)
}

fn spawn(command: &mut Command, start: Instant, timeout: Duration) -> io::Result<Child> {
    for _ in 0..10 {
        let left = remaining(start, timeout)?;
        match command.spawn() {
            // A newly written script can briefly fail with ETXTBSY on Unix.
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => {
                thread::sleep(left.min(SPAWN_RETRY_DELAY));
            }
            result => return result,
        }
    }
    remaining(start, timeout)?;
    command.spawn()
}

fn capture(child: &mut Child, start: Instant, timeout: Duration) -> io::Result<Output> {
    let mut out = child.stdout.take().expect("stdout pipe");
    let mut err = child.stderr.take().expect("stderr pipe");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut buffer = [0; 16 * 1024];
    loop {
        let left = remaining(start, timeout)?;
        if !stdout_open && !stderr_open {
            if let Some(status) = child.try_wait()? {
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            thread::sleep(left.min(EXIT_POLL_INTERVAL));
            continue;
        }
        let mut pipes = [
            PollFd::new(&out, PollFlags::IN),
            PollFd::new(&err, PollFlags::IN),
        ];
        let first = usize::from(!stdout_open);
        let last = 1 + usize::from(stderr_open);
        let timeout = Timespec::try_from(left.min(Duration::from_secs(1)))
            .expect("timeout capped at one second");
        match poll(&mut pipes[first..last], Some(&timeout)) {
            Err(rustix::io::Errno::INTR) => continue,
            result => result?,
        };
        let [out_events, err_events] = pipes.map(|pipe| pipe.revents());
        if stdout_open && !out_events.is_empty() {
            stdout_open = read_pipe(&mut out, &mut stdout, &mut buffer)?;
        }
        if stderr_open && !err_events.is_empty() {
            stderr_open = read_pipe(&mut err, &mut stderr, &mut buffer)?;
        }
    }
}

fn read_pipe(pipe: &mut impl Read, output: &mut Vec<u8>, buffer: &mut [u8]) -> io::Result<bool> {
    match pipe.read(buffer) {
        Ok(0) => Ok(false),
        Ok(read) => {
            output.extend_from_slice(&buffer[..read]);
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(true),
        Err(error) => Err(error),
    }
}
