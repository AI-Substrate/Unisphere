//! The explicit filesystem adapter. Configuration policy lives in the service.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

use unisphere_core::{ConfigReader, ReadFailure};

/// Reads a caller-selected absolute file, consuming at most `max_bytes + 1` bytes.
///
/// The extra byte distinguishes an exact-limit document from an oversized one,
/// including a file that grows after its metadata is inspected. No configuration
/// search, root traversal, path expansion, or writes are performed.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdConfigReader;

impl ConfigReader for StdConfigReader {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
        // The service reports relative paths as invalid configuration. Keep the
        // adapter safe to call directly without consulting the working directory.
        if !path.is_absolute() {
            return Err(ReadFailure::Other);
        }
        let file = File::open(path).map_err(read_failure)?;
        let metadata = file.metadata().map_err(read_failure)?;
        if !metadata.is_file() {
            return Err(ReadFailure::Other);
        }
        if metadata.len() > max_bytes as u64 {
            return Err(ReadFailure::TooLarge);
        }
        read_bounded(file, max_bytes)
    }
}

fn read_bounded(input: impl Read, max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
    let mut bytes = Vec::new();
    input.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(read_failure)?;
    if bytes.len() > max_bytes {
        return Err(ReadFailure::TooLarge);
    }
    Ok(bytes)
}

fn read_failure(error: io::Error) -> ReadFailure {
    match error.kind() {
        io::ErrorKind::NotFound => ReadFailure::NotFound,
        io::ErrorKind::PermissionDenied => ReadFailure::PermissionDenied,
        _ => ReadFailure::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_growing_input_stops_after_the_single_oversize_probe_byte() {
        let mut input = io::Cursor::new(b"longer than its earlier metadata".as_slice());
        assert_eq!(read_bounded(&mut input, 3), Err(ReadFailure::TooLarge));
        assert_eq!(input.position(), 4);
        let mut exact = io::Cursor::new(b"abc".as_slice());
        assert_eq!(read_bounded(&mut exact, 3), Ok(b"abc".to_vec()));
        assert_eq!(exact.position(), 3);
    }

    #[test]
    fn io_categories_are_preserved_without_carrying_os_messages() {
        for (kind, expected) in [
            (io::ErrorKind::NotFound, ReadFailure::NotFound),
            (io::ErrorKind::PermissionDenied, ReadFailure::PermissionDenied),
            (io::ErrorKind::InvalidData, ReadFailure::Other),
        ] {
            assert_eq!(
                read_failure(io::Error::new(kind, "SENSITIVE-OS-MARKER")),
                expected
            );
        }
    }
}
