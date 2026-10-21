use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

enum Job {
    Write(PathBuf, String),
    #[cfg(test)]
    Flush(Sender<()>),
}

static WRITER: OnceLock<Sender<Job>> = OnceLock::new();

fn writer() -> &'static Sender<Job> {
    WRITER.get_or_init(|| {
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
            .expect("spawn persist thread");
        sender
    })
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
/// replaced atomically.
///
/// @example
/// persist::save(file.clone(), format!("{}\n", percent));
pub fn save(file: PathBuf, text: String) {
    let _ = writer().send(Job::Write(file, text));
}

#[cfg(test)]
pub fn flush() {
    let (done, wait) = channel();
    if writer().send(Job::Flush(done)).is_ok() {
        let _ = wait.recv();
    }
}
