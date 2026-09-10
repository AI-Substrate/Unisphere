use super::QueryNameError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use std::{fmt, str::FromStr};

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Stable SHA-256 bytes, never a process-dependent Rust hash.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Hash a domain and components with unambiguous little-endian lengths.
    pub fn framed<'a>(domain: &[u8], components: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut digest = Sha256::new();
        digest.update((domain.len() as u64).to_le_bytes());
        digest.update(domain);
        for component in components {
            digest.update((component.len() as u64).to_le_bytes());
            digest.update(component);
        }
        Self(digest.finalize().into())
    }
}
impl FromStr for Digest {
    type Err = QueryNameError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 {
            return Err(QueryNameError);
        }
        let mut bytes = [0; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = HEX
                .iter()
                .position(|byte| *byte == pair[0])
                .ok_or(QueryNameError)?;
            let low = HEX
                .iter()
                .position(|byte| *byte == pair[1])
                .ok_or(QueryNameError)?;
            bytes[index] = ((high << 4) | low) as u8;
        }
        Ok(Self(bytes))
    }
}
impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = [0; 64];
        for (index, byte) in self.0.iter().copied().enumerate() {
            text[index * 2] = HEX[usize::from(byte >> 4)];
            text[index * 2 + 1] = HEX[usize::from(byte & 15)];
        }
        // ASCII by construction.
        formatter.write_str(std::str::from_utf8(&text).map_err(|_| fmt::Error)?)
    }
}
impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}
impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

query_enum! {
    pub enum EntityKind {
        Source => "source", Session => "session", Turn => "turn", Message => "message",
        Tool => "tool", Event => "event", Branch => "branch", Partition => "partition",
        Group => "group"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId {
    kind: EntityKind,
    digest: Digest,
}
impl EntityId {
    pub const fn new(kind: EntityKind, digest: Digest) -> Self {
        Self { kind, digest }
    }
    pub const fn kind(self) -> EntityKind {
        self.kind
    }
    pub const fn digest(self) -> Digest {
        self.digest
    }
    pub fn derive<'a>(kind: EntityKind, components: impl IntoIterator<Item = &'a [u8]>) -> Self {
        Self::new(
            kind,
            Digest::framed(
                b"unisphere/query-entity/v1",
                std::iter::once(kind.as_str().as_bytes()).chain(components),
            ),
        )
    }
}
impl FromStr for EntityId {
    type Err = QueryNameError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let suffix = value.strip_prefix("q1:").ok_or(QueryNameError)?;
        let (kind, digest) = suffix.split_once(':').ok_or(QueryNameError)?;
        Ok(Self::new(kind.parse()?, digest.parse()?))
    }
}
impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "q1:{}:{}", self.kind, self.digest)
    }
}
impl Serialize for EntityId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for EntityId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(Digest);
impl SourceId {
    pub const fn new(digest: Digest) -> Self {
        Self(digest)
    }
    pub const fn digest(self) -> Digest {
        self.0
    }
    pub fn derive<'a>(components: impl IntoIterator<Item = &'a [u8]>) -> Self {
        Self(Digest::framed(b"unisphere/query-source/v1", components))
    }
    pub const fn entity(self) -> EntityId {
        EntityId::new(EntityKind::Source, self.0)
    }
}
impl FromStr for SourceId {
    type Err = QueryNameError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let id: EntityId = value.parse()?;
        if id.kind() != EntityKind::Source {
            return Err(QueryNameError);
        }
        Ok(Self(id.digest()))
    }
}
impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.entity(), f)
    }
}
impl Serialize for SourceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for SourceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

macro_rules! registry_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, QueryNameError> {
                let value = value.into();
                let valid = !value.is_empty()
                    && value.len() <= 128
                    && value.as_bytes()[0].is_ascii_alphanumeric()
                    && value.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'-' | b'_' | b'.')
                    });
                if !valid {
                    return Err(QueryNameError);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl FromStr for $name {
            type Err = QueryNameError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
registry_id!(AdapterId);
registry_id!(HarnessId);

/// Source-local partition identity; it cannot masquerade as a conversation ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartitionId(EntityId);
impl PartitionId {
    pub fn derive(source: SourceId, native_key: &[u8]) -> Self {
        Self(EntityId::derive(
            EntityKind::Partition,
            [source.digest().bytes().as_slice(), native_key],
        ))
    }
    pub const fn entity(self) -> EntityId {
        self.0
    }
}
impl FromStr for PartitionId {
    type Err = QueryNameError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let id: EntityId = value.parse()?;
        if id.kind() != EntityKind::Partition {
            return Err(QueryNameError);
        }
        Ok(Self(id))
    }
}
impl fmt::Display for PartitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl Serialize for PartitionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for PartitionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
