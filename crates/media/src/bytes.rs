//! Reading numbers out of byte slices without ever indexing past the end.

pub(crate) fn slice(b: &[u8], at: usize, len: usize) -> Option<&[u8]> {
    b.get(at..at.checked_add(len)?)
}

pub(crate) fn tag(b: &[u8], at: usize) -> Option<[u8; 4]> {
    slice(b, at, 4)?.try_into().ok()
}

pub(crate) fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(slice(b, at, 2)?.try_into().ok()?))
}

pub(crate) fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(slice(b, at, 4)?.try_into().ok()?))
}

pub(crate) fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(slice(b, at, 8)?.try_into().ok()?))
}

pub(crate) fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(slice(b, at, 2)?.try_into().ok()?))
}

pub(crate) fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(slice(b, at, 4)?.try_into().ok()?))
}

/// A four-character code as text (non-printable bytes as `?`), for messages.
pub(crate) fn fourcc(t: [u8; 4]) -> String {
    t.iter().map(|b| if (0x20..0x7F).contains(b) { char::from(*b) } else { '?' }).collect()
}
