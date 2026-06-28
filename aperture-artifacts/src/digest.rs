use std::fmt;
use std::fmt::Write as FmtWrite;
use std::str::FromStr;

use crate::error::{ArtifactError, Result};

/// A content digest (sha256).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digest {
    algorithm: DigestAlgorithm,
    hex: Box<str>,
}

impl Digest {
    pub fn from_hash(algorithm: DigestAlgorithm, hash: &[u8]) -> Self {
        let mut hex = String::new();
        hex.reserve_exact(hash.len() * 2);
        for byte in hash {
            let _ = write!(hex, "{byte:02x}");
        }
        Self {
            algorithm,
            hex: hex.into_boxed_str(),
        }
    }

    /// Returns the digest algorithm.
    pub const fn algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    /// Returns the hex digest without the algorithm prefix.
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl FromStr for Digest {
    type Err = ArtifactError;

    /// Parses a digest of the form `sha256:<hex>`.
    fn from_str(value: &str) -> Result<Self> {
        let (algorithm, hex) = value
            .split_once(':')
            .ok_or_else(|| ArtifactError::InvalidDigest(value.to_owned()))?;
        let algorithm = algorithm.parse()?;
        let valid = hex.len() % 2 == 0 && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            return Err(ArtifactError::InvalidDigest(value.to_owned()));
        }
        Ok(Self {
            algorithm,
            hex: hex.to_ascii_lowercase().into_boxed_str(),
        })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { algorithm, hex } = self;
        write!(f, "{algorithm}:{hex}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DigestAlgorithm {
    Sha256,
}

impl DigestAlgorithm {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

impl FromStr for DigestAlgorithm {
    type Err = ArtifactError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "sha256" => Ok(Self::Sha256),
            _ => Err(ArtifactError::InvalidDigest(value.to_owned())),
        }
    }
}

impl fmt::Display for DigestAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
