use std::ops::Range;

pub fn detect_sub_slice<T>(parent: &[T], child: &[T]) -> Option<Range<usize>> {
    // Adapted from `Bytes::slice_ref`.

    // Empty slice and empty Bytes may have their pointers reset
    // so explicitly allow empty slice to be a sub-slice of any slice.
    if child.is_empty() {
        return Some(0..0);
    }

    let bytes_p = parent.as_ptr() as usize;
    let bytes_len = parent.len();

    let sub_p = child.as_ptr() as usize;
    let sub_len = child.len();

    if sub_p < bytes_p {
        return None;
    }

    if sub_p + sub_len > bytes_p + bytes_len {
        return None;
    }

    let sub_offset = sub_p - bytes_p;

    Some(sub_offset..(sub_offset + sub_len))
}
