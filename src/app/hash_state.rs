use crate::app::App;
use crate::commands::types::HashAlgorithm;
use crate::core::document::walk::WalkControl;
use crate::error::HxResult;

const HASH_LOGICAL_CHUNK_BYTES: usize = 64 * 1024;
#[cfg(test)]
pub(crate) const HASH_PROGRESS_STEP_BYTES: u64 = 1024 * 1024;
#[cfg(not(test))]
pub(crate) const HASH_PROGRESS_STEP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashScope {
    EntireFile,
    Selection,
}

impl HashScope {
    pub(crate) fn label(self, start: u64, end: u64) -> String {
        match self {
            Self::EntireFile => "entire file".to_owned(),
            Self::Selection => format!("sel 0x{start:x}-0x{end:x}"),
        }
    }
}

pub(crate) struct HashProgressScan {
    algorithm: HashAlgorithm,
    scope: HashScope,
    start: u64,
    end: u64,
    cursor: u64,
    scanned_display: u64,
    bytes_hashed: u64,
    hasher: Box<dyn digest::DynDigest>,
}

impl HashProgressScan {
    pub(crate) fn new(algorithm: HashAlgorithm, scope: HashScope, start: u64, end: u64) -> Self {
        Self {
            algorithm,
            scope,
            start,
            end,
            cursor: start,
            scanned_display: 0,
            bytes_hashed: 0,
            hasher: crate::exec::make_hasher(algorithm),
        }
    }

    fn display_total(&self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    fn scope_label(&self) -> String {
        self.scope.label(self.start, self.end)
    }
}

impl App {
    pub(crate) fn start_hash_scan(
        &mut self,
        algorithm: HashAlgorithm,
        scope: HashScope,
        start: u64,
        end: u64,
    ) {
        let scan = HashProgressScan::new(algorithm, scope, start, end);
        self.pending_hash_scan = Some(scan);
        self.set_hash_scan_status();
    }

    pub(crate) fn hash_scan_pending(&self) -> bool {
        self.pending_hash_scan.is_some()
    }

    pub(crate) fn cancel_hash_scan(&mut self, message: Option<&str>) {
        self.pending_hash_scan = None;
        if let Some(message) = message {
            self.set_info_status(message);
        }
    }

    pub(crate) fn report_hash_scan_blocked_input(&mut self) {
        self.set_hash_scan_status();
    }

    pub(crate) fn continue_hash_scan(&mut self) -> HxResult<()> {
        let Some(mut scan) = self.pending_hash_scan.take() else {
            return Ok(());
        };

        if scan.cursor > scan.end {
            self.finish_hash_scan(scan);
            return Ok(());
        }

        let step_end = scan
            .cursor
            .saturating_add(HASH_PROGRESS_STEP_BYTES)
            .saturating_sub(1)
            .min(scan.end);
        let display_scanned = step_end.saturating_sub(scan.cursor).saturating_add(1);

        self.document.walk_logical_chunks(
            scan.cursor,
            step_end,
            HASH_LOGICAL_CHUNK_BYTES,
            |chunk| {
                if !chunk.bytes.is_empty() {
                    scan.hasher.update(chunk.bytes);
                    scan.bytes_hashed = scan.bytes_hashed.saturating_add(chunk.bytes.len() as u64);
                }
                Ok(WalkControl::Continue)
            },
        )?;

        scan.scanned_display = scan.scanned_display.saturating_add(display_scanned);
        if step_end >= scan.end {
            self.finish_hash_scan(scan);
            return Ok(());
        }

        scan.cursor = step_end.saturating_add(1);
        self.pending_hash_scan = Some(scan);
        self.set_hash_scan_status();
        Ok(())
    }

    fn finish_hash_scan(&mut self, scan: HashProgressScan) {
        let algorithm = scan.algorithm;
        let scope = scan.scope_label();
        let bytes_hashed = scan.bytes_hashed;
        let hash_bytes = scan.hasher.finalize();
        let hex = hash_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        if bytes_hashed == 0 {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return;
        }

        self.set_hash_result_status(algorithm, scope, hex, bytes_hashed);
    }

    pub(crate) fn set_hash_result_status(
        &mut self,
        algorithm: HashAlgorithm,
        scope: String,
        hex: String,
        bytes_hashed: u64,
    ) {
        if crate::clipboard::copy_text(&hex).is_ok() {
            self.set_info_status(format!(
                "{} [{}]: {} ({} bytes) [copied]",
                algorithm.label(),
                scope,
                hex,
                bytes_hashed
            ));
        } else {
            self.set_info_status(format!(
                "{} [{}]: {} ({} bytes)",
                algorithm.label(),
                scope,
                hex,
                bytes_hashed
            ));
        }
    }

    fn set_hash_scan_status(&mut self) {
        let message = {
            let Some(scan) = self.pending_hash_scan.as_ref() else {
                return;
            };
            let total = scan.display_total();
            let percent = if total == 0 {
                100.0
            } else {
                (scan.scanned_display as f64 / total as f64 * 100.0).min(100.0)
            };
            format!(
                "hashing {} [{}]... {} / {} checked ({percent:.0}%); {} logical hashed; Esc to cancel",
                scan.algorithm.label(),
                scan.scope_label(),
                format_hash_progress_bytes(scan.scanned_display),
                format_hash_progress_bytes(total),
                format_hash_progress_bytes(scan.bytes_hashed)
            )
        };
        self.set_info_status(message);
    }
}

fn format_hash_progress_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} bytes")
    }
}
