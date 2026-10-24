use std::path::{Path, PathBuf};

use super::Mount;

fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes.get(i + 1..i + 4).and_then(|digits| {
            std::str::from_utf8(digits)
                .ok()
                .and_then(|text| u8::from_str_radix(text, 8).ok())
        });
        match (bytes[i], octal) {
            (b'\\', Some(byte)) => {
                out.push(byte);
                i += 4;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn entries(text: &str) -> impl Iterator<Item = (Mount, String)> {
    text.lines().filter_map(|line| {
        let (left, right) = line.split_once(" - ")?;
        let point = left.split(' ').nth(4)?;
        let mut right = right.split(' ');
        let fs_type = right.next()?;
        let source = right.next().unwrap_or_default();
        Some((
            Mount {
                point: PathBuf::from(unescape(point)),
                fs_type: fs_type.to_owned(),
            },
            unescape(source),
        ))
    })
}

pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    entries(text).map(|(mount, _)| mount).collect()
}

fn source_of(text: &str, path: &Path) -> Option<String> {
    entries(text)
        .filter(|(mount, _)| path.starts_with(&mount.point))
        .max_by_key(|(mount, _)| mount.point.components().count())
        .map(|(_, source)| source)
}

fn flag(block: &str) -> Option<bool> {
    ["queue/rotational", "../queue/rotational"]
        .iter()
        .find_map(|flag| std::fs::read_to_string(format!("{block}/{flag}")).ok())
        .map(|text| text.trim() == "1")
}

pub fn rotational(device: u64) -> bool {
    let block = format!(
        "/sys/dev/block/{}:{}",
        rustix::fs::major(device),
        rustix::fs::minor(device)
    );
    flag(&block).unwrap_or(false)
}

pub fn rotational_mount(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    let Some(source) = source_of(&text, path) else {
        return false;
    };
    source
        .split(':')
        .filter(|device| device.starts_with("/dev/"))
        .filter_map(|device| std::fs::canonicalize(device).ok())
        .filter_map(|device| device.file_name().map(|name| name.to_owned()))
        .any(|name| flag(&format!("/sys/class/block/{}", name.to_string_lossy())).unwrap_or(false))
}

pub fn mounts() -> Vec<Mount> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|text| parse_mountinfo(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mountinfo_lines() {
        let text = "22 1 0:21 / /proc rw,nosuid shared:5 - proc proc rw\n\
                    30 1 259:2 / /mnt/my\\040disk rw - ext4 /dev/nvme0n1p2 rw\n";
        let mounts = parse_mountinfo(text);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].fs_type, "proc");
        assert_eq!(mounts[1].point, PathBuf::from("/mnt/my disk"));
    }

    #[test]
    fn finds_the_source_of_the_deepest_mount() {
        let text = "22 1 0:30 / / rw - btrfs /dev/sda2 rw\n\
                    30 22 0:31 / /mnt/pool rw - bcachefs /dev/sdb:/dev/sdc rw\n";
        assert_eq!(
            source_of(text, Path::new("/mnt/pool/data")).as_deref(),
            Some("/dev/sdb:/dev/sdc")
        );
        assert_eq!(
            source_of(text, Path::new("/home/me")).as_deref(),
            Some("/dev/sda2")
        );
    }
}
