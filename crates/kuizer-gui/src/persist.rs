use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{SendError, Sender, channel};

enum Job {
    Write(PathBuf, String),
    #[cfg(test)]
    Flush(Sender<()>),
}

static WRITER: OnceLock<Sender<Job>> = OnceLock::new();

fn writer() -> Option<&'static Sender<Job>> {
    if let Some(sender) = WRITER.get() {
        return Some(sender);
    }
    let (sender, receiver) = channel();
    std::thread::Builder::new()
        .name("kuizer-gui-persist".to_owned())
        .spawn(move || {
            while let Ok(first) = receiver.recv() {
                let mut latest: HashMap<PathBuf, String> = HashMap::new();
                #[cfg(test)]
                let mut flushes = Vec::new();
                for job in std::iter::once(first).chain(receiver.try_iter()) {
                    match job {
                        Job::Write(file, text) => {
                            latest.insert(file, text);
                        }
                        #[cfg(test)]
                        Job::Flush(done) => flushes.push(done),
                    }
                }
                for (file, text) in latest {
                    write(&file, &text);
                }
                #[cfg(test)]
                for done in flushes {
                    let _ = done.send(());
                }
            }
        })
        .ok()?;
    Some(WRITER.get_or_init(|| sender))
}

fn submit(job: Job) {
    let unsent = match writer() {
        Some(sender) => sender.send(job).err().map(|SendError(job)| job),
        None => Some(job),
    };
    match unsent {
        Some(Job::Write(file, text)) => write(&file, &text),
        #[cfg(test)]
        Some(Job::Flush(done)) => {
            let _ = done.send(());
        }
        None => {}
    }
}

fn write(file: &Path, text: &str) {
    if let Some(parent) = file.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let mut temporary = file.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let result = std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, file));
    if let Err(error) = result {
        eprintln!("kuizer-gui: cannot save {}: {error}", file.display());
    }
}

/// @name save
/// @description Queues a small state file for writing on a background thread so the interface
/// never waits on the disk. Writes to the same file collapse to the latest text and the file is
/// replaced atomically. If the thread cannot run, the file is written on the calling thread so
/// the state is not lost.
///
/// @example
/// persist::save(file.clone(), format!("{}\n", percent));
pub fn save(file: PathBuf, text: String) {
    submit(Job::Write(file, text));
}

#[cfg(test)]
pub fn flush() {
    let (done, wait) = channel();
    submit(Job::Flush(done));
    let _ = wait.recv();
}
