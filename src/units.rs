use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ByteSize(pub u64);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid size `{0}`: use bytes or a number with B, KB, MB, GB, TB, KiB, MiB, GiB or TiB")]
pub struct SizeError(String);

impl FromStr for ByteSize {
    type Err = SizeError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let text = input.trim();
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let value: f64 = number.parse().map_err(|_| SizeError(input.to_owned()))?;
        let factor: u64 = match unit.trim().to_ascii_lowercase().as_str() {
            "" | "b" => 1,
            "k" | "kb" => 1_000,
            "m" | "mb" => 1_000_000,
            "g" | "gb" => 1_000_000_000,
            "t" | "tb" => 1_000_000_000_000,
            "kib" => 1 << 10,
            "mib" => 1 << 20,
            "gib" => 1 << 30,
            "tib" => 1 << 40,
            _ => return Err(SizeError(input.to_owned())),
        };
        if !value.is_finite() || value < 0.0 {
            return Err(SizeError(input.to_owned()));
        }
        Ok(Self((value * factor as f64).round() as u64))
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
        let mut value = self.0 as f64;
        let mut unit = 0;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            write!(f, "{} B", self.0)
        } else {
            write!(f, "{value:.1} {}", UNITS[unit])
        }
    }
}

impl Serialize for ByteSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(u64),
            Text(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Number(bytes) => Ok(Self(bytes)),
            Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!("0".parse(), Ok(ByteSize(0)));
        assert_eq!("512".parse(), Ok(ByteSize(512)));
        assert_eq!("10MB".parse(), Ok(ByteSize(10_000_000)));
        assert_eq!("1.5 GiB".parse(), Ok(ByteSize(1_610_612_736)));
        assert_eq!("4kib".parse(), Ok(ByteSize(4096)));
    }

    #[test]
    fn rejects_garbage() {
        assert!("ten".parse::<ByteSize>().is_err());
        assert!("10XB".parse::<ByteSize>().is_err());
        assert!("".parse::<ByteSize>().is_err());
    }

    #[test]
    fn formats_binary_units() {
        assert_eq!(ByteSize(900).to_string(), "900 B");
        assert_eq!(ByteSize(1536).to_string(), "1.5 KiB");
        assert_eq!(ByteSize(3 << 30).to_string(), "3.0 GiB");
    }
}
