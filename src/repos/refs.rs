use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::{KIB, Objects};
use crate::platform;

const PACK_INDEX_HEADER: u64 = 8 + 256 * 4;
const PACK_EXTENSIONS: [&str; 7] = ["idx", "pack", "rev", "bitmap", "keep", "promisor", "mtimes"];

pub(super) struct Refs {
    pub(super) current: Option<String>,
    pub(super) origin_head: Option<String>,
    pub(super) heads: Vec<String>,
    pub(super) names: HashSet<String>,
    pub(super) worktrees: bool,
}

pub(super) struct OnDisk {
    pub(super) refs: Refs,
    pub(super) objects: Objects,
}

impl OnDisk {
    pub(super) fn read(git_dir: &Path) -> Option<Self> {
        let unusual = ["commondir", "reftable", "objects/info/alternates"]
            .iter()
            .any(|name| git_dir.join(name).exists());
        if unusual || !git_dir.join("refs").is_dir() || !git_dir.join("objects").is_dir() {
            return None;
        }
        let config = fs::read_to_string(git_dir.join("config"))
            .ok()?
            .to_ascii_lowercase();
        if config.contains("refstorage") || config.contains("objectformat") {
            return None;
        }
        Some(Self {
            refs: Refs::read(git_dir)?,
            objects: objects_on_disk(&git_dir.join("objects"))?,
        })
    }
}

impl Refs {
    pub(super) fn read(git_dir: &Path) -> Option<Self> {
        let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
        let head = head.trim_end();
        let current = match head.strip_prefix("ref: ") {
            Some(target) => Some(target.strip_prefix("refs/heads/")?.to_owned()),
            None if is_object_id(head) => None,
            None => return None,
        };
        let mut names = HashSet::new();
        let mut origin_head = None;
        for top in ["refs/heads", "refs/remotes/origin"] {
            loose_refs(git_dir, top, &mut names, &mut origin_head)?;
        }
        if let Ok(packed) = fs::read_to_string(git_dir.join("packed-refs")) {
            for line in packed.lines() {
                if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                    continue;
                }
                let (id, name) = line.split_once(' ')?;
                if !is_object_id(id) {
                    return None;
                }
                if name.starts_with("refs/heads/") || name.starts_with("refs/remotes/origin/") {
                    names.insert(name.to_owned());
                }
            }
        }
        let mut heads: Vec<String> = names
            .iter()
            .filter_map(|name| name.strip_prefix("refs/heads/"))
            .map(str::to_owned)
            .collect();
        heads.sort();
        let worktrees = fs::read_dir(git_dir.join("worktrees"))
            .is_ok_and(|mut entries| entries.next().is_some());
        Some(Self {
            current,
            origin_head,
            heads,
            names,
            worktrees,
        })
    }
}

fn loose_refs(
    git_dir: &Path,
    name: &str,
    names: &mut HashSet<String>,
    origin_head: &mut Option<String>,
) -> Option<()> {
    let path = git_dir.join(name);
    let Ok(entries) = fs::read_dir(&path) else {
        return Some(());
    };
    for entry in entries {
        let entry = entry.ok()?;
        let child = format!("{name}/{}", entry.file_name().to_str()?);
        if child.ends_with(".lock") {
            continue;
        }
        if entry.file_type().ok()?.is_dir() {
            loose_refs(git_dir, &child, names, origin_head)?;
            continue;
        }
        let text = fs::read_to_string(entry.path()).ok()?;
        let text = text.trim_end();
        if child == "refs/remotes/origin/HEAD"
            && let Some(target) = text.strip_prefix("ref: ")
        {
            *origin_head = target.strip_prefix("refs/remotes/").map(str::to_owned);
            continue;
        }
        if !is_object_id(text) {
            return None;
        }
        names.insert(child);
    }
    Some(())
}

fn is_object_id(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn loose_id(fan: u8, name: &str) -> Option<[u8; 20]> {
    if name.len() != 38 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut id = [fan; 20];
    for (index, byte) in id[1..].iter_mut().enumerate() {
        *byte = u8::from_str_radix(&name[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(id)
}

fn kib_floor(bytes: u64) -> u64 {
    bytes / KIB * KIB
}

fn stat_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

fn objects_on_disk(dir: &Path) -> Option<Objects> {
    let mut objects = Objects::default();
    let mut loose_bytes = 0;
    let mut garbage_bytes = 0;
    let mut loose_ids = Vec::new();
    for fan in 0..=255u8 {
        let Ok(entries) = fs::read_dir(dir.join(format!("{fan:02x}"))) else {
            continue;
        };
        for entry in entries {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let id = name.to_str().and_then(|name| loose_id(fan, name));
            match (id, entry.metadata()) {
                (Some(id), Ok(metadata)) if metadata.is_file() => {
                    objects.loose += 1;
                    loose_bytes += platform::allocated(&metadata);
                    loose_ids.push(id);
                }
                _ => garbage_bytes += stat_size(&entry.path()),
            }
        }
    }

    let mut groups: BTreeMap<String, Vec<(PathBuf, String)>> = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(dir.join("pack")) {
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = entry.file_name().to_str()?.to_owned();
            if name.starts_with("multi-pack-index") {
                continue;
            }
            match name.rsplit_once('.') {
                Some((stem, extension)) if PACK_EXTENSIONS.contains(&extension) => {
                    groups
                        .entry(stem.to_owned())
                        .or_default()
                        .push((path, extension.to_owned()));
                }
                _ => garbage_bytes += stat_size(&path),
            }
        }
    }
    let mut indexes = Vec::new();
    let mut packed_bytes = 0;
    for files in groups.values() {
        let find = |wanted: &str| {
            files
                .iter()
                .find(|(_, extension)| extension == wanted)
                .map(|(path, _)| path)
        };
        let (Some(pack), Some(index)) = (find("pack"), find("idx")) else {
            garbage_bytes += files.iter().map(|(path, _)| stat_size(path)).sum::<u64>();
            continue;
        };
        indexes.push(PackIndex::open(index)?);
        objects.packs += 1;
        packed_bytes += stat_size(pack) + stat_size(index);
    }

    let mut pending = loose_ids;
    for index in &mut indexes {
        let mut block: Option<(u8, Vec<u8>)> = None;
        let mut missing = Vec::new();
        for id in pending {
            if block.as_ref().is_none_or(|(first, _)| *first != id[0]) {
                block = Some((id[0], index.block(id[0]).ok()?));
            }
            let names = block
                .as_ref()
                .map_or(&[][..], |(_, names)| names.as_slice());
            if sorted_contains(names, &id) {
                objects.prunable += 1;
            } else {
                missing.push(id);
            }
        }
        pending = missing;
    }
    objects.loose_bytes = kib_floor(loose_bytes);
    objects.packed_bytes = kib_floor(packed_bytes);
    objects.garbage_bytes = kib_floor(garbage_bytes);
    Some(objects)
}

struct PackIndex {
    file: File,
    fanout: Vec<u32>,
}

impl PackIndex {
    fn open(path: &Path) -> Option<Self> {
        let mut file = File::open(path).ok()?;
        let mut header = [0; PACK_INDEX_HEADER as usize];
        file.read_exact(&mut header).ok()?;
        if header[..8] != [0xff, b't', b'O', b'c', 0, 0, 0, 2] {
            return None;
        }
        let fanout = header[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_be_bytes(*bytes))
            .collect();
        Some(Self { file, fanout })
    }

    fn block(&mut self, first: u8) -> std::io::Result<Vec<u8>> {
        let first = usize::from(first);
        let low = match first {
            0 => 0,
            _ => self.fanout[first - 1],
        };
        let high = self.fanout[first].max(low);
        let mut names = vec![0; (high - low) as usize * 20];
        self.file
            .seek(SeekFrom::Start(PACK_INDEX_HEADER + u64::from(low) * 20))?;
        self.file.read_exact(&mut names)?;
        Ok(names)
    }
}

fn sorted_contains(names: &[u8], id: &[u8; 20]) -> bool {
    let (mut low, mut high) = (0, names.len() / 20);
    while low < high {
        let middle = low + (high - low) / 2;
        match names[middle * 20..middle * 20 + 20].cmp(id) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}
