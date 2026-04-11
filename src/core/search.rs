use memchr::memmem;

/// Wrapper around `memchr::memmem` so the document layer can swap search
/// strategy without carrying crate-specific details around.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    memmem::find(haystack, needle)
}

pub fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    memmem::rfind(haystack, needle)
}
