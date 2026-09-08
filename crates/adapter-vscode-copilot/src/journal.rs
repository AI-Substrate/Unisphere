//! Replay the native object mutation log, not one chat event per physical line.
use serde_json::{Map, Value};
use unisphere_core::{NativeSnapshot, PipelineError, PipelineErrorKind};

fn malformed() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidData, None)
}

pub(crate) fn reduce(snapshot: &NativeSnapshot) -> Result<Value, PipelineError> {
    // Sparse JS array extensions serialize as null. Bound synthesized nulls by
    // their JSON byte cost, against the supplied raw representation, before any
    // resize. A tiny hostile `i` must not request gigabytes of allocation.
    let mut holes = snapshot.records.iter().try_fold(0usize, |size, record| {
        size.checked_add(record.bytes.len()).ok_or_else(malformed)
    })? / 4;
    let mut state = None;
    for record in &snapshot.records {
        let Value::Object(mut entry) =
            serde_json::from_slice(&record.bytes).map_err(|_| malformed())?
        else {
            return Err(malformed());
        };
        let kind = entry
            .get("kind")
            .and_then(Value::as_u64)
            .ok_or_else(malformed)?;
        if kind == 0 {
            state = Some(entry.remove("v").ok_or_else(malformed)?);
            continue;
        }
        if !matches!(kind, 1..=3) {
            return Err(malformed());
        }
        let root = state.as_mut().ok_or_else(malformed)?;
        let Value::Array(path) = entry.remove("k").ok_or_else(malformed)? else {
            return Err(malformed());
        };
        if !path
            .iter()
            .all(|part| part.is_string() || part.as_u64().is_some())
        {
            return Err(malformed());
        }
        if path.is_empty() {
            // Native Set/Delete explicitly ignore an empty path. Native Push
            // addresses an `undefined` property, not the root: reject it.
            if kind == 2 {
                return Err(malformed());
            }
            continue;
        }
        let (leaf, parents) = path.split_last().ok_or_else(malformed)?;
        let mut parent = root;
        for segment in parents {
            parent = child_mut(parent, segment).ok_or_else(malformed)?;
        }
        match kind {
            1 => set(
                parent,
                leaf,
                Some(entry.remove("v").ok_or_else(malformed)?),
                &mut holes,
            )?,
            2 => push(parent, leaf, &mut entry, &mut holes)?,
            3 => set(parent, leaf, None, &mut holes)?,
            _ => unreachable!(),
        }
    }
    state.ok_or_else(malformed)
}

fn object_key(segment: &Value) -> String {
    match segment {
        Value::String(key) => key.clone(),
        _ => segment.to_string(),
    }
}

fn array_index(segment: &Value) -> Option<usize> {
    if let Some(index) = segment.as_u64() {
        return usize::try_from(index).ok();
    }
    // JS accepts canonical decimal property strings as array indices. Other
    // array properties are not serialized chat data and are not supported.
    let text = segment.as_str()?;
    let index: usize = text.parse().ok()?;
    (index.to_string() == text).then_some(index)
}

fn child_mut<'a>(parent: &'a mut Value, segment: &Value) -> Option<&'a mut Value> {
    match parent {
        Value::Object(object) => object.get_mut(&object_key(segment)),
        Value::Array(array) => array.get_mut(array_index(segment)?),
        _ => None,
    }
}

fn extend(array: &mut Vec<Value>, length: usize, holes: &mut usize) -> Result<(), PipelineError> {
    if length > u32::MAX as usize {
        return Err(malformed());
    }
    let added = length.saturating_sub(array.len());
    *holes = holes
        .checked_sub(added)
        .ok_or_else(|| PipelineError::new(PipelineErrorKind::BatchLimit, None))?;
    array.resize(length, Value::Null);
    Ok(())
}

fn set(
    parent: &mut Value,
    segment: &Value,
    value: Option<Value>,
    holes: &mut usize,
) -> Result<(), PipelineError> {
    match parent {
        Value::Object(object) => {
            let key = object_key(segment);
            if let Some(value) = value {
                object.insert(key, value);
            } else {
                // Native Delete is assignment to undefined. Removing object
                // keys, or replacing array slots with null below, has the same
                // JSON projection and subsequent supported path behavior.
                object.remove(&key);
            }
        }
        Value::Array(array) => {
            let index = array_index(segment)
                .filter(|index| *index < u32::MAX as usize)
                .ok_or_else(malformed)?;
            if index >= array.len() {
                extend(array, index.checked_add(1).ok_or_else(malformed)?, holes)?;
            }
            array[index] = value.unwrap_or(Value::Null);
        }
        _ => return Err(malformed()),
    }
    Ok(())
}

fn push(
    parent: &mut Value,
    segment: &Value,
    entry: &mut Map<String, Value>,
    holes: &mut usize,
) -> Result<(), PipelineError> {
    // A missing or falsy leaf starts an array; never auto-create intermediates.
    let slot = match parent {
        Value::Object(object) => object.entry(object_key(segment)).or_insert(Value::Null),
        Value::Array(array) => {
            let index = array_index(segment)
                .filter(|index| *index < u32::MAX as usize)
                .ok_or_else(malformed)?;
            if index >= array.len() {
                extend(array, index.checked_add(1).ok_or_else(malformed)?, holes)?;
            }
            &mut array[index]
        }
        _ => return Err(malformed()),
    };
    if slot.is_null()
        || slot == &Value::Bool(false)
        || slot.as_f64() == Some(0.0)
        || slot.as_str() == Some("")
    {
        *slot = Value::Array(Vec::new());
    }
    let array = slot.as_array_mut().ok_or_else(malformed)?;
    if let Some(index) = entry.remove("i") {
        let length = index
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(malformed)?;
        extend(array, length, holes)?;
    }
    if let Some(value) = entry.remove("v") {
        let Value::Array(mut values) = value else {
            return Err(malformed());
        };
        if array
            .len()
            .checked_add(values.len())
            .is_none_or(|len| len > u32::MAX as usize)
        {
            return Err(malformed());
        }
        array.append(&mut values);
    }
    Ok(())
}
