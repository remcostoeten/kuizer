use std::path::PathBuf;

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

pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let fs_type = right.split(' ').next()?;
            Some(Mount {
                point: PathBuf::from(unescape(point)),
                fs_type: fs_type.to_owned(),
            })
        })
        .collect()
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
}
