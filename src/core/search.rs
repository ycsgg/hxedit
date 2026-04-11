/// Streaming KMP matcher used by forward searches over file chunks.
#[derive(Debug, Clone)]
pub struct KmpSearcher {
    pattern: Vec<u8>,
    failure: Vec<usize>,
    matched: usize,
}

impl KmpSearcher {
    pub fn new(pattern: Vec<u8>) -> Self {
        let failure = build_failure(&pattern);
        Self {
            pattern,
            failure,
            matched: 0,
        }
    }

    pub fn reset(&mut self) {
        self.matched = 0;
    }

    pub fn feed(&mut self, byte: u8) -> bool {
        while self.matched > 0 && self.pattern[self.matched] != byte {
            self.matched = self.failure[self.matched - 1];
        }
        if !self.pattern.is_empty() && self.pattern[self.matched] == byte {
            self.matched += 1;
            if self.matched == self.pattern.len() {
                self.matched = self.failure[self.matched - 1];
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.pattern.len()
    }
}

fn build_failure(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0; pattern.len()];
    let mut len = 0;
    for idx in 1..pattern.len() {
        while len > 0 && pattern[len] != pattern[idx] {
            len = failure[len - 1];
        }
        if pattern[len] == pattern[idx] {
            len += 1;
            failure[idx] = len;
        }
    }
    failure
}
