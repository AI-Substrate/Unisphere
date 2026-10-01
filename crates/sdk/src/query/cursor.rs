use std::str::FromStr;

use unisphere_core::query::{
    CursorBinding, CursorMismatchReason, Digest, QueryFailure, QueryFailureCode, RecoveryAction,
};

const VERSION: &str = "q1";

pub(crate) fn encode(binding: CursorBinding, next_index: usize) -> Result<String, QueryFailure> {
    let index = next_index.to_string();
    let checksum = checksum(binding, index.as_bytes());
    Ok(format!(
        "{VERSION}.{}.{}.{}.{checksum}",
        binding.view_digest, binding.query_digest, index
    ))
}

pub(crate) fn decode(
    value: &str,
    expected: CursorBinding,
    row_count: usize,
) -> Result<usize, QueryFailure> {
    let mut parts = value.split('.');
    let version = parts.next();
    let view = parts.next();
    let query = parts.next();
    let index = parts.next();
    let supplied_checksum = parts.next();
    if version != Some(VERSION)
        || parts.next().is_some()
        || view.is_none()
        || query.is_none()
        || index.is_none()
        || supplied_checksum.is_none()
    {
        return Err(invalid_cursor());
    }
    let binding = CursorBinding {
        view_digest: Digest::from_str(view.unwrap()).map_err(|_| invalid_cursor())?,
        query_digest: Digest::from_str(query.unwrap()).map_err(|_| invalid_cursor())?,
    };
    let index_text = index.unwrap();
    let index = index_text.parse::<usize>().map_err(|_| invalid_cursor())?;
    let supplied_checksum =
        Digest::from_str(supplied_checksum.unwrap()).map_err(|_| invalid_cursor())?;
    if checksum(binding, index_text.as_bytes()) != supplied_checksum {
        return Err(invalid_cursor());
    }
    binding.validate(expected.query_digest, expected.view_digest)?;
    if index == 0 || index > row_count {
        return Err(QueryFailure::stale_cursor(
            CursorMismatchReason::QueryOptionsChanged,
        ));
    }
    Ok(index)
}

fn checksum(binding: CursorBinding, index: &[u8]) -> Digest {
    Digest::framed(
        b"unisphere/query-cursor/v1",
        [
            binding.view_digest.bytes().as_slice(),
            binding.query_digest.bytes().as_slice(),
            index,
        ],
    )
}

fn invalid_cursor() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::InvalidArgument,
        RecoveryAction::StartFreshQuery,
    )
}
