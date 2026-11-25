use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use super::{Candidate, Member, Seen, Stage};
use crate::scan::{Cancel, Issue};

const SAMPLE: u64 = 16 * 1024;
pub(super) const SAMPLED: u64 = 3 * SAMPLE;
const CHUNK: usize = 1024 * 1024;
const COMPARE_CHUNK: usize = 64 * 1024;

thread_local! {
    static HASH_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(CHUNK));
    static COMPARE_BUFFERS: RefCell<(Vec<u8>, Vec<u8>)> = RefCell::new((
        Vec::with_capacity(COMPARE_CHUNK),
        Vec::with_capacity(COMPARE_CHUNK),
    ));
}

pub(super) fn same_size_candidates(mut seen: Vec<Seen>) -> Vec<Candidate> {
    seen.sort_unstable_by(|a, b| a.size.cmp(&b.size).then_with(|| a.path.cmp(&b.path)));
    let mut candidates = Vec::new();
    let mut group: Vec<Seen> = Vec::new();
    for file in seen {
        if group.first().is_some_and(|first| first.size != file.size) {
            keep_group(&mut group, &mut candidates);
        }
        group.push(file);
    }
    keep_group(&mut group, &mut candidates);
    candidates
}

fn keep_group(group: &mut Vec<Seen>, candidates: &mut Vec<Candidate>) {
    let files = std::mem::take(group);
    if files.len() > 1 {
        let merged = merge_links(files);
        if merged.len() > 1 {
            candidates.extend(merged);
        }
    }
}

fn merge_links(files: Vec<Seen>) -> Vec<Candidate> {
    let mut by_inode: HashMap<(u64, u64), usize> = HashMap::new();
    let mut merged: Vec<Candidate> = Vec::new();
    for file in files {
        if let Some(identity) = file.identity
            && let Some(&index) = by_inode.get(&identity)
        {
            merged[index].member.links.push(file.path);
            continue;
        }
        if let Some(identity) = file.identity {
            by_inode.insert(identity, merged.len());
        }
        merged.push(Candidate {
            size: file.size,
            allocated: file.allocated,
            member: Member {
                path: file.path,
                links: Vec::new(),
                modified: file.modified,
            },
        });
    }
    for candidate in &mut merged {
        candidate.member.links.sort();
    }
    merged
}

fn interrupted() -> io::Error {
    io::Error::from(io::ErrorKind::Interrupted)
}

fn advise_sequential(file: &File) {
    #[cfg(target_os = "linux")]
    let _ = rustix::fs::fadvise(file, 0, None, rustix::fs::Advice::Sequential);
    #[cfg(not(target_os = "linux"))]
    let _ = file;
}

pub(super) fn sample_hash(path: &Path, size: u64) -> io::Result<blake3::Hash> {
    let file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; SAMPLE as usize];
    for offset in [0, (size - SAMPLE) / 2, size - SAMPLE] {
        file.read_exact_at(&mut buffer, offset)?;
        hasher.update(&buffer);
    }
    Ok(hasher.finalize())
}

pub(super) fn full_hash(path: &Path, size: u64, cancel: &Cancel) -> io::Result<blake3::Hash> {
    let mut file = File::open(path)?;
    if size > CHUNK as u64 {
        advise_sequential(&file);
    }
    let mut hasher = blake3::Hasher::new();
    HASH_BUFFER.with_borrow_mut(|buffer| {
        buffer.resize(CHUNK.min(usize::try_from(size).unwrap_or(CHUNK) + 1), 0);
        loop {
            if cancel.is_cancelled() {
                return Err(interrupted());
            }
            let read = file.read(buffer)?;
            if read == 0 {
                return Ok(hasher.finalize());
            }
            hasher.update(&buffer[..read]);
        }
    })
}

pub(super) fn same_bytes(a: &Path, b: &Path, cancel: &Cancel) -> io::Result<bool> {
    let mut left = io::BufReader::with_capacity(COMPARE_CHUNK, File::open(a)?);
    let mut right = io::BufReader::with_capacity(COMPARE_CHUNK, File::open(b)?);
    COMPARE_BUFFERS.with_borrow_mut(|(left_buf, right_buf)| {
        left_buf.resize(COMPARE_CHUNK, 0);
        right_buf.resize(COMPARE_CHUNK, 0);
        loop {
            if cancel.is_cancelled() {
                return Err(interrupted());
            }
            let read = left.read(&mut left_buf[..])?;
            if read == 0 {
                return Ok(right.read(&mut right_buf[..1])? == 0);
            }
            match right.read_exact(&mut right_buf[..read]) {
                Ok(()) if left_buf[..read] == right_buf[..read] => {}
                Ok(()) => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(error) => return Err(error),
            }
        }
    })
}

pub(super) fn in_parallel<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    cancel: &Cancel,
    work: impl Fn(&T) -> R + Sync,
) -> Option<Vec<R>> {
    if items.is_empty() {
        return Some(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let mut results: Vec<(usize, R)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.clamp(1, items.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        if cancel.is_cancelled() {
                            break done;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break done;
                        };
                        done.push((index, work(item)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("hashing thread panicked"))
            .collect()
    });
    if cancel.is_cancelled() {
        return None;
    }
    results.sort_unstable_by_key(|(index, _)| *index);
    Some(results.into_iter().map(|(_, result)| result).collect())
}

type Hashed = Vec<(blake3::Hash, Vec<Candidate>)>;

pub(super) fn bucket(
    candidates: Vec<Candidate>,
    hashes: Vec<io::Result<blake3::Hash>>,
    issues: &mut Vec<Issue>,
) -> Vec<((u64, blake3::Hash), Vec<Candidate>)> {
    let mut keyed: HashMap<(u64, blake3::Hash), Vec<Candidate>> = HashMap::new();
    for (candidate, hash) in candidates.into_iter().zip(hashes) {
        match hash {
            Ok(hash) => keyed
                .entry((candidate.size, hash))
                .or_default()
                .push(candidate),
            Err(error) => issues.push(Issue::from_io(candidate.member.path, &error)),
        }
    }
    keyed
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .collect()
}

pub(super) struct Pass<'a> {
    pub(super) workers: usize,
    pub(super) cancel: &'a Cancel,
}

impl Pass<'_> {
    pub(super) fn full(
        &self,
        mut flat: Vec<Candidate>,
        issues: &mut Vec<Issue>,
        on_stage: &mut dyn FnMut(Stage),
    ) -> Option<Hashed> {
        flat.sort_by_key(|candidate| std::cmp::Reverse(candidate.size));
        on_stage(Stage::Hashing { files: flat.len() });
        let hashes = in_parallel(&flat, self.workers, self.cancel, |candidate| {
            full_hash(&candidate.member.path, candidate.size, self.cancel)
        })?;
        Some(
            bucket(flat, hashes, issues)
                .into_iter()
                .map(|((_, hash), members)| (hash, members))
                .collect(),
        )
    }

    pub(super) fn sampled(
        &self,
        flat: Vec<Candidate>,
        issues: &mut Vec<Issue>,
        on_stage: &mut dyn FnMut(Stage),
    ) -> Option<Vec<Candidate>> {
        on_stage(Stage::Hashing { files: flat.len() });
        let samples = in_parallel(&flat, self.workers, self.cancel, |candidate| {
            sample_hash(&candidate.member.path, candidate.size)
        })?;
        Some(
            bucket(flat, samples, issues)
                .into_iter()
                .flat_map(|(_, members)| members)
                .collect(),
        )
    }
}

struct Split {
    clusters: Vec<Vec<usize>>,
    issues: Vec<Issue>,
}

fn split_by_bytes(members: &[Candidate], cancel: &Cancel) -> Split {
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut issues = Vec::new();
    'members: for (index, member) in members.iter().enumerate() {
        for cluster in &mut clusters {
            match same_bytes(
                &members[cluster[0]].member.path,
                &member.member.path,
                cancel,
            ) {
                Ok(true) => {
                    cluster.push(index);
                    continue 'members;
                }
                Ok(false) => {}
                Err(error) => {
                    if error.kind() != io::ErrorKind::Interrupted {
                        issues.push(Issue::from_io(member.member.path.clone(), &error));
                    }
                    continue 'members;
                }
            }
        }
        clusters.push(vec![index]);
    }
    clusters.retain(|cluster| cluster.len() > 1);
    Split { clusters, issues }
}

pub(super) fn verified(
    hashed: Hashed,
    workers: usize,
    cancel: &Cancel,
    issues: &mut Vec<Issue>,
) -> Option<Hashed> {
    let splits = in_parallel(&hashed, workers, cancel, |(_, members)| {
        split_by_bytes(members, cancel)
    })?;
    let mut groups = Vec::new();
    for ((hash, members), split) in hashed.into_iter().zip(splits) {
        issues.extend(split.issues);
        let mut slots: Vec<Option<Candidate>> = members.into_iter().map(Some).collect();
        for cluster in split.clusters {
            let cluster = cluster
                .into_iter()
                .filter_map(|index| slots[index].take())
                .collect();
            groups.push((hash, cluster));
        }
    }
    Some(groups)
}
