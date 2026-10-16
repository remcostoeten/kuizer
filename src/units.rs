use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ByteSize(pub u64);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid size `{0}`: use bytes or a number with B, KB, MB, GB, TB, KiB, MiB, GiB or TiB")]
pub struct SizeError(String);

impl FromStr for ByteSize {
    type Err = SizeError;

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "sizes accept fractions like 1.5GiB and are checked to be finite and non-negative"
    )]
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
    #[allow(
        clippy::cast_precision_loss,
        reason = "display rounds to one decimal, so precision past 2^53 bytes does not matter"
    )]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Idle(pub Duration);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid duration `{0}`: use a whole number with d, w, mo or y, e.g. 60d")]
pub struct IdleError(String);

impl FromStr for Idle {
    type Err = IdleError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let text = input.trim();
        let split = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let count: u64 = number.parse().map_err(|_| IdleError(input.to_owned()))?;
        let days: u64 = match unit.trim().to_ascii_lowercase().as_str() {
            "d" => 1,
            "w" => 7,
            "mo" => 30,
            "y" => 365,
            _ => return Err(IdleError(input.to_owned())),
        };
        count
            .checked_mul(days * 86_400)
            .map(|seconds| Self(Duration::from_secs(seconds)))
            .ok_or_else(|| IdleError(input.to_owned()))
    }
}

impl fmt::Display for Idle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}d", self.0.as_secs() / 86_400)
    }
}

/// @name age
/// @description Formats the time elapsed since `time` as a short age such as `today`, `5d`,
/// `3w`, `4mo` or `2y`, or `-` when the time is unknown or in the future.
///
/// @example
/// let label = units::age(candidate.newest);
pub fn age(time: Option<SystemTime>) -> String {
    let Some(days) = time
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .map(|elapsed| elapsed.as_secs() / 86_400)
    else {
        return "-".to_owned();
    };
    match days {
        0 => "today".to_owned(),
        1..=13 => format!("{days}d"),
        14..=59 => format!("{}w", days / 7),
        60..=729 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
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

    #[test]
    fn parses_idle_durations() {
        let days = |n: u64| Ok(Idle(Duration::from_secs(n * 86_400)));
        assert_eq!("60d".parse(), days(60));
        assert_eq!("2w".parse(), days(14));
        assert_eq!("6mo".parse(), days(180));
        assert_eq!(" 1y ".parse(), days(365));
        assert!("60".parse::<Idle>().is_err());
        assert!("1.5d".parse::<Idle>().is_err());
        assert!("d".parse::<Idle>().is_err());
        assert!("3h".parse::<Idle>().is_err());
    }
}
