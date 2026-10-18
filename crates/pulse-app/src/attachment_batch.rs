//! Pack queued Data without waiting or crossing control boundaries.
use pulse_ipc::attach::AttachError;
use pulse_term::session::StreamRecord;
use tokio::sync::mpsc;

/// Active body <= fragment; one lookahead retains a barrier or non-fitting record.
/// The lease's single repair permit remains attached to the retained record.
pub(super) fn coalesce(
    record: &mut StreamRecord,
    output: &mut mpsc::Receiver<StreamRecord>,
    lookahead: &mut Option<StreamRecord>,
    fragment: usize,
) -> Result<(), AttachError> {
    if lookahead.is_some() {
        return Ok(());
    }
    // At most the original eight queued records, even with concurrent refills.
    for _ in 0..8 {
        let StreamRecord::Data {
            start,
            bytes,
            boundary,
        } = record
        else {
            break;
        };
        if bytes.len() >= fragment {
            break;
        }
        let Ok(next) = output.try_recv() else {
            break;
        };
        match next {
            StreamRecord::Data {
                start: next_start,
                bytes: next_bytes,
                boundary: next_boundary,
            } if next_bytes.len() <= fragment - bytes.len() => {
                if bytes.is_empty()
                    || next_bytes.is_empty()
                    || start.checked_add(bytes.len() as u64) != Some(boundary.utf8_bytes)
                    || next_start != boundary.utf8_bytes
                    || next_start.checked_add(next_bytes.len() as u64)
                        != Some(next_boundary.utf8_bytes)
                    || next_boundary.parsed_bytes < boundary.parsed_bytes
                    || next_boundary.revision < boundary.revision
                    || next_boundary.size != boundary.size
                    || next_boundary.lifecycle != boundary.lifecycle
                    || next_boundary.source_is_utf8 != boundary.source_is_utf8
                {
                    return Err(AttachError::Invalid);
                }
                if bytes.capacity() < fragment {
                    bytes.reserve_exact(fragment - bytes.len());
                }
                bytes.extend_from_slice(&next_bytes);
                *boundary = next_boundary;
            }
            other => {
                *lookahead = Some(other);
                break;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "attachment_batch_tests.rs"]
mod tests;
