pub const BYTE_VALUE_COUNT: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteStatEntry {
    pub byte: u8,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteStats {
    counts: [u64; BYTE_VALUE_COUNT],
    logical_bytes: u64,
}

impl Default for ByteStats {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteStats {
    pub fn new() -> Self {
        Self {
            counts: [0; BYTE_VALUE_COUNT],
            logical_bytes: 0,
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        let mut local = [[0_u64; BYTE_VALUE_COUNT]; 4];
        let chunks = bytes.chunks_exact(4);
        let remainder = chunks.remainder();
        for chunk in chunks {
            local[0][chunk[0] as usize] += 1;
            local[1][chunk[1] as usize] += 1;
            local[2][chunk[2] as usize] += 1;
            local[3][chunk[3] as usize] += 1;
        }
        for (index, byte) in remainder.iter().copied().enumerate() {
            local[index][byte as usize] += 1;
        }

        for table in local {
            for (byte, count) in table.into_iter().enumerate() {
                if count != 0 {
                    self.counts[byte] = self.counts[byte].saturating_add(count);
                }
            }
        }
        self.logical_bytes = self.logical_bytes.saturating_add(bytes.len() as u64);
    }

    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }

    pub fn count(&self, byte: u8) -> u64 {
        self.counts[byte as usize]
    }

    pub fn unique_count(&self) -> usize {
        self.counts.iter().filter(|count| **count != 0).count()
    }

    pub fn entropy_bits_per_byte(&self) -> f64 {
        if self.logical_bytes == 0 {
            return 0.0;
        }
        let total = self.logical_bytes as f64;
        self.counts
            .iter()
            .copied()
            .filter(|count| *count != 0)
            .map(|count| {
                let p = count as f64 / total;
                -p * p.log2()
            })
            .sum()
    }

    pub fn ascii_printable_count(&self) -> u64 {
        self.count_range(0x20, 0x7e)
    }

    pub fn ascii_whitespace_count(&self) -> u64 {
        [b'\t', b'\n', b'\x0b', b'\x0c', b'\r', b' ']
            .into_iter()
            .map(|byte| self.count(byte))
            .sum()
    }

    pub fn ascii_control_count(&self) -> u64 {
        self.count_range(0x00, 0x1f)
            .saturating_add(self.count(0x7f))
    }

    pub fn high_nibble_buckets(&self) -> [u64; 16] {
        let mut buckets = [0_u64; 16];
        for (byte, count) in self.counts.iter().copied().enumerate() {
            buckets[byte >> 4] = buckets[byte >> 4].saturating_add(count);
        }
        buckets
    }

    pub fn top_bytes(&self, limit: usize) -> Vec<ByteStatEntry> {
        let mut entries = self
            .counts
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count != 0)
            .map(|(byte, count)| ByteStatEntry {
                byte: byte as u8,
                count,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.byte.cmp(&right.byte))
        });
        entries.truncate(limit);
        entries
    }

    fn count_range(&self, start: u8, end: u8) -> u64 {
        self.counts[start as usize..=end as usize]
            .iter()
            .copied()
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_unique_and_entropy() {
        let mut stats = ByteStats::new();
        stats.update(&[0x00, 0x00, 0xff, b'A']);

        assert_eq!(stats.logical_bytes(), 4);
        assert_eq!(stats.count(0x00), 2);
        assert_eq!(stats.count(0xff), 1);
        assert_eq!(stats.count(b'A'), 1);
        assert_eq!(stats.unique_count(), 3);
        assert!((stats.entropy_bits_per_byte() - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn uniform_byte_values_have_full_entropy() {
        let bytes = (0_u8..=255).collect::<Vec<_>>();
        let mut stats = ByteStats::new();
        stats.update(&bytes);

        assert_eq!(stats.logical_bytes(), BYTE_VALUE_COUNT as u64);
        assert_eq!(stats.unique_count(), BYTE_VALUE_COUNT);
        assert!((stats.entropy_bits_per_byte() - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_bytes_sort_by_count_then_byte() {
        let mut stats = ByteStats::new();
        stats.update(&[3, 2, 2, 1, 1, 1, 0, 0, 0]);

        let top = stats.top_bytes(3);
        assert_eq!(top[0], ByteStatEntry { byte: 0, count: 3 });
        assert_eq!(top[1], ByteStatEntry { byte: 1, count: 3 });
        assert_eq!(top[2], ByteStatEntry { byte: 2, count: 2 });
    }

    #[test]
    fn ascii_and_nibble_summaries() {
        let mut stats = ByteStats::new();
        stats.update(&[0x00, b'\n', b' ', b'A', 0x7f, 0xf0, 0xff]);

        assert_eq!(stats.ascii_printable_count(), 2);
        assert_eq!(stats.ascii_whitespace_count(), 2);
        assert_eq!(stats.ascii_control_count(), 3);
        let buckets = stats.high_nibble_buckets();
        assert_eq!(buckets[0], 2);
        assert_eq!(buckets[2], 1);
        assert_eq!(buckets[4], 1);
        assert_eq!(buckets[7], 1);
        assert_eq!(buckets[15], 2);
    }
}
