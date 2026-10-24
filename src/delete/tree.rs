use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, OFlags, ResolveFlags, fstat, openat, openat2, statat,
    unlinkat,
};
use rustix::io::Errno;

const OPEN_DIR: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

static OPENAT2_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

enum Parent {
    Dir(Arc<Node>),
    Root(OwnedFd),
}

struct Node {
    fd: OwnedFd,
    parent: Parent,
    name: CString,
    pending: AtomicUsize,
}

impl Node {
    fn parent_fd(&self) -> BorrowedFd<'_> {
        match &self.parent {
            Parent::Dir(parent) => parent.fd.as_fd(),
            Parent::Root(fd) => fd.as_fd(),
        }
    }
}

struct Task {
    parent: Arc<Node>,
    name: CString,
}

struct Queue {
    stack: Vec<Task>,
    running: usize,
}

struct Run {
    root: PathBuf,
    device: u64,
    workers: usize,
    spawned: AtomicUsize,
    queue: Mutex<Queue>,
    wake: Condvar,
    error: Mutex<Option<io::Error>>,
}

type Scope<'scope, 'env> = &'scope thread::Scope<'scope, 'env>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Run {
    fn path_of(&self, node: &Node) -> PathBuf {
        let mut names = Vec::new();
        let mut current = node;
        while let Parent::Dir(parent) = &current.parent {
            names.push(current.name.as_c_str());
            current = parent;
        }
        let mut path = self.root.clone();
        for name in names.into_iter().rev() {
            path.push(OsStr::from_bytes(name.to_bytes()));
        }
        path
    }

    fn fail(&self, path: &Path, error: &io::Error) {
        lock(&self.error).get_or_insert_with(|| {
            io::Error::new(error.kind(), format!("{}: {error}", path.display()))
        });
    }

    fn has_failed(&self) -> bool {
        lock(&self.error).is_some()
    }

    fn push<'scope, 'env>(&'scope self, task: Task, scope: Scope<'scope, 'env>) {
        let queued = {
            let mut queue = lock(&self.queue);
            queue.stack.push(task);
            queue.stack.len()
        };
        self.wake.notify_one();
        if queued >= 2 && self.spawned.load(Ordering::Relaxed) + 1 < self.workers {
            self.spawned.fetch_add(1, Ordering::Relaxed);
            let _ = thread::Builder::new()
                .name("kuizer-unlink".to_owned())
                .spawn_scoped(scope, move || self.work(scope));
        }
    }

    fn next(&self) -> Option<Task> {
        let mut queue = lock(&self.queue);
        loop {
            if let Some(task) = queue.stack.pop() {
                queue.running += 1;
                return Some(task);
            }
            if queue.running == 0 {
                return None;
            }
            queue = self
                .wake
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn done(&self) {
        let mut queue = lock(&self.queue);
        queue.running -= 1;
        if queue.running == 0 && queue.stack.is_empty() {
            self.wake.notify_all();
        }
    }

    fn work<'scope, 'env>(&'scope self, scope: Scope<'scope, 'env>) {
        while let Some(task) = self.next() {
            self.execute(task, scope);
            self.done();
        }
    }

    fn execute<'scope, 'env>(&'scope self, task: Task, scope: Scope<'scope, 'env>) {
        let Task { parent, name } = task;
        let opened = open_below(&parent.fd, name.as_c_str())
            .and_then(|fd| fstat(&fd).map(|stat| (fd, stat.st_dev)));
        match opened {
            Ok((fd, device)) if device == self.device => {
                let node = Arc::new(Node {
                    fd,
                    parent: Parent::Dir(parent),
                    name,
                    pending: AtomicUsize::new(1),
                });
                self.scan(&node, scope);
            }
            Ok(_) | Err(Errno::XDEV) => {
                let path = self
                    .path_of(&parent)
                    .join(OsStr::from_bytes(name.to_bytes()));
                let error = io::Error::other("is a mount point, not removed");
                self.fail(&path, &error);
                self.complete(parent);
            }
            Err(errno) => {
                let path = self
                    .path_of(&parent)
                    .join(OsStr::from_bytes(name.to_bytes()));
                self.fail(&path, &errno.into());
                self.complete(parent);
            }
        }
    }

    fn scan<'scope, 'env>(&'scope self, node: &Arc<Node>, scope: Scope<'scope, 'env>) {
        let dir = node
            .fd
            .try_clone()
            .and_then(|fd| Dir::new(fd).map_err(io::Error::from));
        let mut dir = match dir {
            Ok(dir) => dir,
            Err(error) => {
                self.fail(&self.path_of(node), &error);
                self.complete(Arc::clone(node));
                return;
            }
        };
        while let Some(entry) = dir.read() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(errno) => {
                    self.fail(&self.path_of(node), &errno.into());
                    break;
                }
            };
            let name = entry.file_name();
            if name == c"." || name == c".." {
                continue;
            }
            if is_directory(node, entry.file_type(), name) {
                node.pending.fetch_add(1, Ordering::Relaxed);
                let task = Task {
                    parent: Arc::clone(node),
                    name: name.to_owned(),
                };
                self.push(task, scope);
            } else if let Err(errno) = unlinkat(&node.fd, name, AtFlags::empty())
                && errno != Errno::NOENT
            {
                let path = self.path_of(node).join(OsStr::from_bytes(name.to_bytes()));
                self.fail(&path, &errno.into());
            }
        }
        self.complete(Arc::clone(node));
    }

    fn complete(&self, mut node: Arc<Node>) {
        loop {
            if node.pending.fetch_sub(1, Ordering::AcqRel) != 1 {
                return;
            }
            if let Err(errno) = unlinkat(node.parent_fd(), node.name.as_c_str(), AtFlags::REMOVEDIR)
                && errno != Errno::NOENT
                && !(errno == Errno::NOTEMPTY && self.has_failed())
            {
                self.fail(&self.path_of(&node), &errno.into());
            }
            let next = match &node.parent {
                Parent::Dir(parent) => Arc::clone(parent),
                Parent::Root(_) => return,
            };
            node = next;
        }
    }
}

fn is_directory(node: &Node, kind: FileType, name: &CStr) -> bool {
    match kind {
        FileType::Directory => true,
        FileType::Unknown => statat(&node.fd, name, AtFlags::SYMLINK_NOFOLLOW)
            .is_ok_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::Directory),
        _ => false,
    }
}

fn open_below(parent: impl AsFd, name: &CStr) -> Result<OwnedFd, Errno> {
    if !OPENAT2_UNAVAILABLE.load(Ordering::Relaxed) {
        match openat2(
            &parent,
            name,
            OPEN_DIR,
            Mode::empty(),
            ResolveFlags::NO_XDEV,
        ) {
            Err(Errno::NOSYS | Errno::PERM) => OPENAT2_UNAVAILABLE.store(true, Ordering::Relaxed),
            opened => return opened,
        }
    }
    openat(parent, name, OPEN_DIR, Mode::empty())
}

/// @name remove
/// @description Removes a file, a symlink or a whole directory tree, unlinking the contents of
/// different directories on several threads. Symlinks are removed, never followed. When
/// `expected` holds the (device, inode) recorded at scan time, the entry is removed only if it
/// still has that identity, so a parent folder swapped for a symlink cannot redirect the removal.
/// A directory on another device than the tree root, and any mount point (bind mounts included
/// where the kernel supports `openat2`), is left alone and reported as an error, which
/// `std::fs::remove_dir_all` does not do.
///
/// @example
/// remove(Path::new("/home/me/app/node_modules"), Some((dev, ino)), 4)?;
pub fn remove(path: &Path, expected: Option<(u64, u64)>, workers: usize) -> io::Result<()> {
    remove_on(path, expected, workers, None)
}

fn changed() -> io::Error {
    io::Error::other("path changed since the scan, not removed")
}

fn remove_on(
    path: &Path,
    expected: Option<(u64, u64)>,
    workers: usize,
    device: Option<u64>,
) -> io::Result<()> {
    let (Some(name), Some(parent)) = (path.file_name(), path.parent()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path has no parent directory",
        ));
    };
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))?;
    let parent_fd = openat(
        CWD,
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let seen = statat(&parent_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    let identity = (seen.st_dev, seen.st_ino);
    let wanted = expected.unwrap_or(identity);
    if identity != wanted {
        return Err(changed());
    }
    if FileType::from_raw_mode(seen.st_mode) != FileType::Directory {
        return unlinkat(&parent_fd, name.as_c_str(), AtFlags::empty()).map_err(io::Error::from);
    }
    let fd = open_below(&parent_fd, name.as_c_str())?;
    let stat = fstat(&fd)?;
    if (stat.st_dev, stat.st_ino) != wanted {
        return Err(changed());
    }
    let run = Run {
        root: path.to_path_buf(),
        device: device.unwrap_or(stat.st_dev),
        workers: workers.max(1),
        spawned: AtomicUsize::new(0),
        queue: Mutex::new(Queue {
            stack: Vec::new(),
            running: 1,
        }),
        wake: Condvar::new(),
        error: Mutex::new(None),
    };
    let root = Arc::new(Node {
        fd,
        parent: Parent::Root(parent_fd),
        name,
        pending: AtomicUsize::new(1),
    });
    thread::scope(|scope| {
        run.scan(&root, scope);
        run.done();
        run.work(scope);
    });
    lock(&run.error).take().map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, symlink};

    use super::*;

    fn build(root: &Path, width: usize, depth: usize) -> usize {
        fs::create_dir_all(root).unwrap();
        let mut count = 0;
        for index in 0..width {
            fs::write(root.join(format!("f{index}")), b"x").unwrap();
            count += 1;
        }
        if depth > 0 {
            for index in 0..width {
                count += build(&root.join(format!("d{index}")), width, depth - 1);
            }
        }
        count
    }

    #[test]
    fn removes_a_deep_tree_and_leaves_its_neighbours() {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let tree = base.join("node_modules");
        build(&tree, 6, 3);
        fs::write(base.join("keep.txt"), b"x").unwrap();

        remove(&tree, None, 4).unwrap();

        assert!(!tree.exists());
        assert!(base.join("keep.txt").exists());
    }

    #[test]
    fn symlinks_are_unlinked_and_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let outside = base.join("outside");
        build(&outside, 3, 1);
        let tree = base.join("tree");
        build(&tree, 3, 1);
        symlink(&outside, tree.join("d0/escape")).unwrap();
        symlink(outside.join("f0"), tree.join("link-to-file")).unwrap();

        remove(&tree, None, 4).unwrap();
        assert!(!tree.exists());
        assert!(outside.join("f0").exists());
        assert!(outside.join("d0/f0").exists());

        let link = base.join("alone");
        symlink(&outside, &link).unwrap();
        remove(&link, None, 4).unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(outside.join("f0").exists());
    }

    #[test]
    fn other_devices_are_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let tree = base.join("tree");
        build(&tree, 2, 1);
        let wrong = fs::symlink_metadata(&tree).unwrap().dev() + 1;

        let error = remove_on(&tree, None, 4, Some(wrong)).unwrap_err();

        assert!(error.to_string().contains("mount point"));
        assert!(tree.join("d0/f0").exists());
        assert!(tree.join("d1/f1").exists());
        assert!(!tree.join("f0").exists());
    }

    #[test]
    fn a_changed_identity_removes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let tree = base.join("tree");
        build(&tree, 2, 1);
        let file = base.join("file");
        fs::write(&file, b"x").unwrap();
        let scanned = |path: &Path| {
            let metadata = fs::symlink_metadata(path).unwrap();
            (metadata.dev(), metadata.ino() + 1)
        };

        let error = remove(&tree, Some(scanned(&tree)), 4).unwrap_err();
        assert!(error.to_string().contains("changed"));
        assert!(tree.join("f0").exists());
        assert!(tree.join("d1/f1").exists());

        assert!(remove(&file, Some(scanned(&file)), 4).is_err());
        assert!(file.exists());

        let identity = |path: &Path| {
            let metadata = fs::symlink_metadata(path).unwrap();
            Some((metadata.dev(), metadata.ino()))
        };
        remove(&tree, identity(&tree), 4).unwrap();
        remove(&file, identity(&file), 4).unwrap();
        assert!(!tree.exists());
        assert!(!file.exists());
    }
}
