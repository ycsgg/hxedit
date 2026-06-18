use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rhai::{Blob, Dynamic, Engine, EvalAltResult, Position, INT};

use crate::commands::types::HashAlgorithm;
use crate::core::document::Document;
use crate::error::{HxError, HxResult};
use crate::exec::{
    execute_batch, DeleteKind, ExecArtifact, ExecBatchOptions, ExecCommand, ExecOffset, ExecRange,
    ExecScope, ExecSearchDirection, ExecState, RangeSpace, SearchSelect,
};
use crate::util::parse::parse_hex_stream;

const DEFAULT_MAX_OPERATIONS: u64 = 2_000_000;
const DEFAULT_MAX_EXEC_CALLS: usize = 100_000;
const DEFAULT_MAX_READ_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_MAX_SINGLE_READ: u64 = 64 * 1024 * 1024;
const DEFAULT_MAX_BLOB_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScriptBudget {
    max_operations: u64,
    max_exec_calls: usize,
    max_read_bytes: u64,
    max_single_read: u64,
    max_blob_bytes: u64,
}

impl Default for ScriptBudget {
    fn default() -> Self {
        Self {
            max_operations: DEFAULT_MAX_OPERATIONS,
            max_exec_calls: DEFAULT_MAX_EXEC_CALLS,
            max_read_bytes: DEFAULT_MAX_READ_BYTES,
            max_single_read: DEFAULT_MAX_SINGLE_READ,
            max_blob_bytes: DEFAULT_MAX_BLOB_BYTES,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScriptReport {
    pub summaries: Vec<String>,
    pub undo_steps: Vec<crate::exec::ExecStep>,
    pub exec_calls: usize,
    pub bytes_read: u64,
    pub saved: bool,
}

pub(crate) struct ScriptRunResult {
    pub document: Document,
    pub state: ExecState,
    pub report: ScriptReport,
}

pub(crate) struct ScriptRunFailure {
    pub document: Document,
    pub state: ExecState,
    pub report: ScriptReport,
    pub error: HxError,
}

pub(crate) fn run_script(
    path: &Path,
    document: Document,
    state: ExecState,
) -> HxResult<ScriptRunResult> {
    let source = std::fs::read_to_string(path)?;
    match run_script_source(path, &source, document, state) {
        Ok(result) => Ok(result),
        Err(failure) => Err(failure.error),
    }
}

pub(crate) fn run_script_source(
    path: &Path,
    source: &str,
    document: Document,
    state: ExecState,
) -> Result<ScriptRunResult, Box<ScriptRunFailure>> {
    let budget = ScriptBudget::default();
    let script_dir = script_dir(path);
    let host = Rc::new(RefCell::new(ScriptHost::new(
        document, state, budget, script_dir,
    )));
    let mut engine = Engine::new();
    engine.set_max_operations(budget.max_operations);
    register_api(&mut engine, Rc::clone(&host));

    let eval_result = engine.eval::<()>(source);

    drop(engine);
    let host = Rc::try_unwrap(host)
        .map_err(|_| {
            Box::new(ScriptRunFailure {
                document: Document::from_memory_bytes(
                    "<script-host-borrowed>".into(),
                    Vec::new(),
                    &crate::config::Config::default(),
                ),
                state: ExecState::new(0, None),
                report: ScriptReport::default(),
                error: HxError::CommandError("script host is still borrowed".to_owned()),
            })
        })?
        .into_inner();
    let result = host.into_result();

    match eval_result {
        Ok(()) => Ok(result),
        Err(err) => Err(Box::new(ScriptRunFailure {
            document: result.document,
            state: result.state,
            report: result.report,
            error: HxError::CommandError(format!("script {}: {err}", path.display())),
        })),
    }
}

struct ScriptHost {
    document: Document,
    state: ExecState,
    script_dir: PathBuf,
    budget: ScriptBudget,
    report: ScriptReport,
}

impl ScriptHost {
    fn new(
        document: Document,
        state: ExecState,
        budget: ScriptBudget,
        script_dir: PathBuf,
    ) -> Self {
        Self {
            document,
            state,
            script_dir,
            budget,
            report: ScriptReport::default(),
        }
    }

    fn into_result(self) -> ScriptRunResult {
        ScriptRunResult {
            document: self.document,
            state: self.state,
            report: self.report,
        }
    }

    fn resolve_script_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.script_dir.join(path)
        }
    }

    fn execute(&mut self, command: ExecCommand) -> HxResult<crate::exec::ExecOutcome> {
        if self.report.exec_calls >= self.budget.max_exec_calls {
            return Err(HxError::CommandError(format!(
                "script exceeded exec call budget ({})",
                self.budget.max_exec_calls
            )));
        }
        self.report.exec_calls += 1;

        let report = execute_batch(
            &mut self.document,
            &mut self.state,
            &[command],
            ExecBatchOptions::default(),
        )?;

        for outcome in &report.outcomes {
            self.report.summaries.push(outcome.summary.clone());
        }
        if report.saved {
            self.report.undo_steps.clear();
            self.report.saved = true;
        }
        self.report.undo_steps.extend(report.undo_steps);

        if let Some(error) = report.error {
            return Err(HxError::CommandError(error));
        }

        report
            .outcomes
            .into_iter()
            .last()
            .ok_or_else(|| HxError::CommandError("script command produced no outcome".to_owned()))
    }

    fn execute_unit(&mut self, command: ExecCommand) -> RhaiResult<()> {
        self.execute(command).map(|_| ()).map_err(to_rhai_error)
    }

    fn execute_read(&mut self, scope: ExecScope, requested_len: Option<u64>) -> RhaiResult<Blob> {
        if let Some(len) = requested_len {
            self.check_read_request(len).map_err(to_rhai_error)?;
        }
        let outcome = self
            .execute(ExecCommand::Read { scope })
            .map_err(to_rhai_error)?;
        let bytes = outcome_bytes(outcome)?;
        self.record_read(bytes.len()).map_err(to_rhai_error)?;
        Ok(bytes)
    }

    fn execute_hash_hex(&mut self, algorithm: &str, scope: ExecScope) -> RhaiResult<String> {
        let algorithm = HashAlgorithm::parse(algorithm)
            .ok_or_else(|| HxError::InvalidHashAlgorithm(algorithm.to_owned()))
            .map_err(to_rhai_error)?;
        let outcome = self
            .execute(ExecCommand::Hash { algorithm, scope })
            .map_err(to_rhai_error)?;
        outcome_text(outcome)
    }

    fn execute_search(
        &mut self,
        pattern: Blob,
        direction: ExecSearchDirection,
        select: SearchSelect,
    ) -> RhaiResult<INT> {
        self.check_blob_len(pattern.len(), "search pattern")
            .map_err(to_rhai_error)?;
        let outcome = self
            .execute(ExecCommand::Search {
                pattern,
                direction,
                select,
            })
            .map_err(to_rhai_error)?;
        match outcome.cursor {
            Some(offset) => u64_to_int(offset),
            None => Ok(-1),
        }
    }

    fn check_blob_len(&self, len: usize, label: &str) -> HxResult<()> {
        if len as u64 > self.budget.max_blob_bytes {
            return Err(HxError::CommandError(format!(
                "{label} exceeds script blob budget ({} bytes)",
                self.budget.max_blob_bytes
            )));
        }
        Ok(())
    }

    fn check_read_request(&self, len: u64) -> HxResult<()> {
        if len > self.budget.max_single_read {
            return Err(HxError::CommandError(format!(
                "script read request exceeds single-read budget ({} bytes)",
                self.budget.max_single_read
            )));
        }
        Ok(())
    }

    fn record_read(&mut self, len: usize) -> HxResult<()> {
        self.report.bytes_read =
            self.report
                .bytes_read
                .checked_add(len as u64)
                .ok_or_else(|| {
                    HxError::CommandError("script read byte counter overflowed".to_owned())
                })?;
        if self.report.bytes_read > self.budget.max_read_bytes {
            return Err(HxError::CommandError(format!(
                "script exceeded total read budget ({} bytes)",
                self.budget.max_read_bytes
            )));
        }
        Ok(())
    }
}

type SharedHost = Rc<RefCell<ScriptHost>>;
type RhaiResult<T> = Result<T, Box<EvalAltResult>>;

fn register_api(engine: &mut Engine, host: SharedHost) {
    let hx_hex_host = Rc::clone(&host);
    engine.register_fn("hx_hex", move |input: &str| -> RhaiResult<Blob> {
        let bytes = parse_hex_stream(input).map_err(to_rhai_error)?;
        hx_hex_host
            .borrow()
            .check_blob_len(bytes.len(), "hex literal")
            .map_err(to_rhai_error)?;
        Ok(bytes)
    });

    let hx_ascii_host = Rc::clone(&host);
    engine.register_fn("hx_ascii", move |input: &str| -> RhaiResult<Blob> {
        let bytes = input.as_bytes().to_vec();
        hx_ascii_host
            .borrow()
            .check_blob_len(bytes.len(), "ascii literal")
            .map_err(to_rhai_error)?;
        Ok(bytes)
    });

    let cursor_host = Rc::clone(&host);
    engine.register_fn("hx_cursor", move || -> RhaiResult<INT> {
        u64_to_int(cursor_host.borrow().state.cursor)
    });

    let len_display_host = Rc::clone(&host);
    engine.register_fn("hx_len_display", move || -> RhaiResult<INT> {
        u64_to_int(len_display_host.borrow().document.len())
    });

    let len_logical_host = Rc::clone(&host);
    engine.register_fn("hx_len_logical", move || -> RhaiResult<INT> {
        u64_to_int(len_logical_host.borrow().document.visible_len())
    });

    let goto_host = Rc::clone(&host);
    engine.register_fn("hx_goto", move |offset: INT| -> RhaiResult<()> {
        let offset = int_to_u64(offset, "offset")?;
        goto_host.borrow_mut().execute_unit(ExecCommand::Goto {
            target: ExecOffset::Absolute(offset),
        })
    });

    let goto_end_host = Rc::clone(&host);
    engine.register_fn("hx_goto_end", move || -> RhaiResult<()> {
        goto_end_host.borrow_mut().execute_unit(ExecCommand::Goto {
            target: ExecOffset::End,
        })
    });

    let select_host = Rc::clone(&host);
    engine.register_fn(
        "hx_select_display",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "selection start")?;
            let len = int_to_u64(len, "selection length")?;
            select_host.borrow_mut().execute_unit(ExecCommand::Select {
                range: ExecRange::display(start, len),
            })
        },
    );

    let select_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_select_logical",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "selection start")?;
            let len = int_to_u64(len, "selection length")?;
            select_logical_host
                .borrow_mut()
                .execute_unit(ExecCommand::Select {
                    range: ExecRange::logical(start, len),
                })
        },
    );

    let clear_selection_host = Rc::clone(&host);
    engine.register_fn("hx_clear_selection", move || -> RhaiResult<()> {
        clear_selection_host
            .borrow_mut()
            .execute_unit(ExecCommand::ClearSelection)
    });

    let has_selection_host = Rc::clone(&host);
    engine.register_fn("hx_has_selection", move || -> bool {
        has_selection_host.borrow().state.selection.is_some()
    });

    let selection_start_host = Rc::clone(&host);
    engine.register_fn("hx_selection_start", move || -> RhaiResult<INT> {
        let selection = selection_start_host
            .borrow()
            .state
            .selection
            .ok_or(HxError::MissingSelection)
            .map_err(to_rhai_error)?;
        u64_to_int(selection.range.start)
    });

    let selection_len_host = Rc::clone(&host);
    engine.register_fn("hx_selection_len", move || -> RhaiResult<INT> {
        let selection = selection_len_host
            .borrow()
            .state
            .selection
            .ok_or(HxError::MissingSelection)
            .map_err(to_rhai_error)?;
        u64_to_int(selection.range.len)
    });

    let selection_space_host = Rc::clone(&host);
    engine.register_fn("hx_selection_space", move || -> RhaiResult<String> {
        let selection = selection_space_host
            .borrow()
            .state
            .selection
            .ok_or(HxError::MissingSelection)
            .map_err(to_rhai_error)?;
        Ok(match selection.range.space {
            RangeSpace::Display => "display",
            RangeSpace::Logical => "logical",
        }
        .to_owned())
    });

    let read_host = Rc::clone(&host);
    engine.register_fn(
        "hx_read_display",
        move |start: INT, len: INT| -> RhaiResult<Blob> {
            let start = int_to_u64(start, "read start")?;
            let len = int_to_u64(len, "read length")?;
            read_host
                .borrow_mut()
                .execute_read(ExecScope::Range(ExecRange::display(start, len)), Some(len))
        },
    );

    let read_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_read_logical",
        move |start: INT, len: INT| -> RhaiResult<Blob> {
            let start = int_to_u64(start, "read start")?;
            let len = int_to_u64(len, "read length")?;
            read_logical_host
                .borrow_mut()
                .execute_read(ExecScope::Range(ExecRange::logical(start, len)), Some(len))
        },
    );

    let read_selection_host = Rc::clone(&host);
    engine.register_fn("hx_read_selection", move || -> RhaiResult<Blob> {
        let mut host = read_selection_host.borrow_mut();
        let requested_len = host.state.selection.map(|selection| selection.range.len);
        host.execute_read(ExecScope::Selection, requested_len)
    });

    let search_host = Rc::clone(&host);
    engine.register_fn("hx_search", move |pattern: Blob| -> RhaiResult<INT> {
        search_host.borrow_mut().execute_search(
            pattern,
            ExecSearchDirection::Forward,
            SearchSelect::None,
        )
    });

    let search_forward_host = Rc::clone(&host);
    engine.register_fn(
        "hx_search_forward",
        move |pattern: Blob| -> RhaiResult<INT> {
            search_forward_host.borrow_mut().execute_search(
                pattern,
                ExecSearchDirection::Forward,
                SearchSelect::None,
            )
        },
    );

    let search_backward_host = Rc::clone(&host);
    engine.register_fn(
        "hx_search_backward",
        move |pattern: Blob| -> RhaiResult<INT> {
            search_backward_host.borrow_mut().execute_search(
                pattern,
                ExecSearchDirection::Backward,
                SearchSelect::None,
            )
        },
    );

    let search_forward_select_host = Rc::clone(&host);
    engine.register_fn(
        "hx_search_forward_select",
        move |pattern: Blob| -> RhaiResult<INT> {
            search_forward_select_host.borrow_mut().execute_search(
                pattern,
                ExecSearchDirection::Forward,
                SearchSelect::Match,
            )
        },
    );

    let search_backward_select_host = Rc::clone(&host);
    engine.register_fn(
        "hx_search_backward_select",
        move |pattern: Blob| -> RhaiResult<INT> {
            search_backward_select_host.borrow_mut().execute_search(
                pattern,
                ExecSearchDirection::Backward,
                SearchSelect::Match,
            )
        },
    );

    let hash_display_host = Rc::clone(&host);
    engine.register_fn(
        "hx_hash_display_hex",
        move |start: INT, len: INT, algorithm: &str| -> RhaiResult<String> {
            let start = int_to_u64(start, "hash start")?;
            let len = int_to_u64(len, "hash length")?;
            hash_display_host
                .borrow_mut()
                .execute_hash_hex(algorithm, ExecScope::Range(ExecRange::display(start, len)))
        },
    );

    let hash_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_hash_logical_hex",
        move |start: INT, len: INT, algorithm: &str| -> RhaiResult<String> {
            let start = int_to_u64(start, "hash start")?;
            let len = int_to_u64(len, "hash length")?;
            hash_logical_host
                .borrow_mut()
                .execute_hash_hex(algorithm, ExecScope::Range(ExecRange::logical(start, len)))
        },
    );

    let hash_selection_host = Rc::clone(&host);
    engine.register_fn(
        "hx_hash_selection_hex",
        move |algorithm: &str| -> RhaiResult<String> {
            hash_selection_host
                .borrow_mut()
                .execute_hash_hex(algorithm, ExecScope::Selection)
        },
    );

    let hash_all_host = Rc::clone(&host);
    engine.register_fn(
        "hx_hash_all_hex",
        move |algorithm: &str| -> RhaiResult<String> {
            hash_all_host
                .borrow_mut()
                .execute_hash_hex(algorithm, ExecScope::All)
        },
    );

    let hash_host = Rc::clone(&host);
    engine.register_fn(
        "hx_hash_hex",
        move |algorithm: &str| -> RhaiResult<String> {
            let scope = active_scope_or_all(&hash_host.borrow().state);
            hash_host.borrow_mut().execute_hash_hex(algorithm, scope)
        },
    );

    let delete_display_host = Rc::clone(&host);
    engine.register_fn(
        "hx_delete_display",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "delete start")?;
            let len = int_to_u64(len, "delete length")?;
            delete_display_host
                .borrow_mut()
                .execute_unit(ExecCommand::Delete {
                    scope: ExecScope::Range(ExecRange::display(start, len)),
                    kind: DeleteKind::Tombstone,
                })
        },
    );

    let delete_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_delete_logical",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "delete start")?;
            let len = int_to_u64(len, "delete length")?;
            delete_logical_host
                .borrow_mut()
                .execute_unit(ExecCommand::Delete {
                    scope: ExecScope::Range(ExecRange::logical(start, len)),
                    kind: DeleteKind::Tombstone,
                })
        },
    );

    let delete_selection_host = Rc::clone(&host);
    engine.register_fn("hx_delete_selection", move || -> RhaiResult<()> {
        delete_selection_host
            .borrow_mut()
            .execute_unit(ExecCommand::Delete {
                scope: ExecScope::Selection,
                kind: DeleteKind::Tombstone,
            })
    });

    let delete_real_display_host = Rc::clone(&host);
    engine.register_fn(
        "hx_delete_real_display",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "delete start")?;
            let len = int_to_u64(len, "delete length")?;
            delete_real_display_host
                .borrow_mut()
                .execute_unit(ExecCommand::Delete {
                    scope: ExecScope::Range(ExecRange::display(start, len)),
                    kind: DeleteKind::Real,
                })
        },
    );

    let delete_real_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_delete_real_logical",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "delete start")?;
            let len = int_to_u64(len, "delete length")?;
            delete_real_logical_host
                .borrow_mut()
                .execute_unit(ExecCommand::Delete {
                    scope: ExecScope::Range(ExecRange::logical(start, len)),
                    kind: DeleteKind::Real,
                })
        },
    );

    let delete_real_selection_host = Rc::clone(&host);
    engine.register_fn("hx_delete_real_selection", move || -> RhaiResult<()> {
        delete_real_selection_host
            .borrow_mut()
            .execute_unit(ExecCommand::Delete {
                scope: ExecScope::Selection,
                kind: DeleteKind::Real,
            })
    });

    let xor_display_host = Rc::clone(&host);
    engine.register_fn(
        "hx_xor_display",
        move |start: INT, len: INT, key: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "xor start")?;
            let len = int_to_u64(len, "xor length")?;
            let key = int_to_u8(key, "xor key")?;
            xor_display_host
                .borrow_mut()
                .execute_unit(ExecCommand::XorInPlace {
                    scope: ExecScope::Range(ExecRange::display(start, len)),
                    key,
                })
        },
    );

    let xor_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_xor_logical",
        move |start: INT, len: INT, key: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "xor start")?;
            let len = int_to_u64(len, "xor length")?;
            let key = int_to_u8(key, "xor key")?;
            xor_logical_host
                .borrow_mut()
                .execute_unit(ExecCommand::XorInPlace {
                    scope: ExecScope::Range(ExecRange::logical(start, len)),
                    key,
                })
        },
    );

    let xor_selection_host = Rc::clone(&host);
    engine.register_fn("hx_xor_selection", move |key: INT| -> RhaiResult<()> {
        let key = int_to_u8(key, "xor key")?;
        xor_selection_host
            .borrow_mut()
            .execute_unit(ExecCommand::XorInPlace {
                scope: ExecScope::Selection,
                key,
            })
    });

    let export_display_host = Rc::clone(&host);
    engine.register_fn(
        "hx_export_display",
        move |start: INT, len: INT, path: &str| -> RhaiResult<()> {
            let start = int_to_u64(start, "export start")?;
            let len = int_to_u64(len, "export length")?;
            let path = export_display_host.borrow().resolve_script_path(path);
            export_display_host
                .borrow_mut()
                .execute_unit(ExecCommand::ExportBinary {
                    scope: ExecScope::Range(ExecRange::display(start, len)),
                    path,
                })
        },
    );

    let export_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_export_logical",
        move |start: INT, len: INT, path: &str| -> RhaiResult<()> {
            let start = int_to_u64(start, "export start")?;
            let len = int_to_u64(len, "export length")?;
            let path = export_logical_host.borrow().resolve_script_path(path);
            export_logical_host
                .borrow_mut()
                .execute_unit(ExecCommand::ExportBinary {
                    scope: ExecScope::Range(ExecRange::logical(start, len)),
                    path,
                })
        },
    );

    let export_selection_host = Rc::clone(&host);
    engine.register_fn("hx_export_selection", move |path: &str| -> RhaiResult<()> {
        let path = export_selection_host.borrow().resolve_script_path(path);
        export_selection_host
            .borrow_mut()
            .execute_unit(ExecCommand::ExportBinary {
                scope: ExecScope::Selection,
                path,
            })
    });

    let replace_all_host = Rc::clone(&host);
    engine.register_fn(
        "hx_replace_all",
        move |needle: Blob, replacement: Blob, allow_resize: bool, force: bool| -> RhaiResult<()> {
            replace_all_host
                .borrow()
                .check_blob_len(needle.len(), "replace needle")
                .map_err(to_rhai_error)?;
            replace_all_host
                .borrow()
                .check_blob_len(replacement.len(), "replace bytes")
                .map_err(to_rhai_error)?;
            replace_all_host
                .borrow_mut()
                .execute_unit(ExecCommand::Replace {
                    scope: ExecScope::All,
                    needle,
                    replacement,
                    allow_resize,
                    force,
                })
        },
    );

    let replace_selection_host = Rc::clone(&host);
    engine.register_fn(
        "hx_replace_selection",
        move |needle: Blob, replacement: Blob, allow_resize: bool, force: bool| -> RhaiResult<()> {
            replace_selection_host
                .borrow()
                .check_blob_len(needle.len(), "replace needle")
                .map_err(to_rhai_error)?;
            replace_selection_host
                .borrow()
                .check_blob_len(replacement.len(), "replace bytes")
                .map_err(to_rhai_error)?;
            replace_selection_host
                .borrow_mut()
                .execute_unit(ExecCommand::Replace {
                    scope: ExecScope::Selection,
                    needle,
                    replacement,
                    allow_resize,
                    force,
                })
        },
    );

    let replace_display_host = Rc::clone(&host);
    engine.register_fn(
        "hx_replace_display",
        move |start: INT,
              len: INT,
              needle: Blob,
              replacement: Blob,
              allow_resize: bool,
              force: bool|
              -> RhaiResult<()> {
            let start = int_to_u64(start, "replace start")?;
            let len = int_to_u64(len, "replace length")?;
            replace_display_host
                .borrow()
                .check_blob_len(needle.len(), "replace needle")
                .map_err(to_rhai_error)?;
            replace_display_host
                .borrow()
                .check_blob_len(replacement.len(), "replace bytes")
                .map_err(to_rhai_error)?;
            replace_display_host
                .borrow_mut()
                .execute_unit(ExecCommand::Replace {
                    scope: ExecScope::Range(ExecRange::display(start, len)),
                    needle,
                    replacement,
                    allow_resize,
                    force,
                })
        },
    );

    let replace_logical_host = Rc::clone(&host);
    engine.register_fn(
        "hx_replace_logical",
        move |start: INT,
              len: INT,
              needle: Blob,
              replacement: Blob,
              allow_resize: bool,
              force: bool|
              -> RhaiResult<()> {
            let start = int_to_u64(start, "replace start")?;
            let len = int_to_u64(len, "replace length")?;
            replace_logical_host
                .borrow()
                .check_blob_len(needle.len(), "replace needle")
                .map_err(to_rhai_error)?;
            replace_logical_host
                .borrow()
                .check_blob_len(replacement.len(), "replace bytes")
                .map_err(to_rhai_error)?;
            replace_logical_host
                .borrow_mut()
                .execute_unit(ExecCommand::Replace {
                    scope: ExecScope::Range(ExecRange::logical(start, len)),
                    needle,
                    replacement,
                    allow_resize,
                    force,
                })
        },
    );

    let overwrite_host = Rc::clone(&host);
    engine.register_fn(
        "hx_overwrite",
        move |offset: INT, bytes: Blob| -> RhaiResult<()> {
            let offset = int_to_u64(offset, "overwrite offset")?;
            overwrite_host
                .borrow()
                .check_blob_len(bytes.len(), "overwrite bytes")
                .map_err(to_rhai_error)?;
            overwrite_host
                .borrow_mut()
                .execute_unit(ExecCommand::Overwrite {
                    offset: ExecOffset::Absolute(offset),
                    bytes,
                })
        },
    );

    let insert_host = Rc::clone(&host);
    engine.register_fn(
        "hx_insert",
        move |offset: INT, bytes: Blob| -> RhaiResult<()> {
            let offset = int_to_u64(offset, "insert offset")?;
            insert_host
                .borrow()
                .check_blob_len(bytes.len(), "insert bytes")
                .map_err(to_rhai_error)?;
            insert_host.borrow_mut().execute_unit(ExecCommand::Insert {
                offset: ExecOffset::Absolute(offset),
                bytes,
            })
        },
    );

    let fill_host = Rc::clone(&host);
    engine.register_fn(
        "hx_fill",
        move |offset: INT, pattern: Blob, len: INT| -> RhaiResult<()> {
            let offset = int_to_u64(offset, "fill offset")?;
            let len = int_to_u64(len, "fill length")?;
            fill_host
                .borrow()
                .check_blob_len(pattern.len(), "fill pattern")
                .map_err(to_rhai_error)?;
            fill_host.borrow_mut().execute_unit(ExecCommand::Fill {
                offset: ExecOffset::Absolute(offset),
                pattern,
                len,
            })
        },
    );

    let save_host = Rc::clone(&host);
    engine.register_fn("hx_save", move || -> RhaiResult<()> {
        save_host
            .borrow_mut()
            .execute_unit(ExecCommand::Save { path: None })
    });

    let save_as_host = Rc::clone(&host);
    engine.register_fn("hx_save_as", move |path: &str| -> RhaiResult<()> {
        let path = save_as_host.borrow().resolve_script_path(path);
        save_as_host
            .borrow_mut()
            .execute_unit(ExecCommand::Save { path: Some(path) })
    });
}

fn script_dir(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn active_scope_or_all(state: &ExecState) -> ExecScope {
    if state.selection.is_some() {
        ExecScope::Selection
    } else {
        ExecScope::All
    }
}

fn outcome_bytes(outcome: crate::exec::ExecOutcome) -> RhaiResult<Blob> {
    outcome
        .artifacts
        .into_iter()
        .find_map(|artifact| match artifact {
            ExecArtifact::Bytes(bytes) => Some(bytes),
            ExecArtifact::Text(_) | ExecArtifact::File(_) => None,
        })
        .ok_or_else(|| to_rhai_error(HxError::CommandError("script expected bytes".to_owned())))
}

fn outcome_text(outcome: crate::exec::ExecOutcome) -> RhaiResult<String> {
    outcome
        .artifacts
        .into_iter()
        .find_map(|artifact| match artifact {
            ExecArtifact::Text(text) => Some(text),
            ExecArtifact::Bytes(_) | ExecArtifact::File(_) => None,
        })
        .ok_or_else(|| to_rhai_error(HxError::CommandError("script expected text".to_owned())))
}

fn int_to_u64(value: INT, label: &str) -> RhaiResult<u64> {
    u64::try_from(value)
        .map_err(|_| to_rhai_error(HxError::CommandError(format!("{label} must be >= 0"))))
}

fn int_to_u8(value: INT, label: &str) -> RhaiResult<u8> {
    u8::try_from(value).map_err(|_| {
        to_rhai_error(HxError::CommandError(format!(
            "{label} must be between 0 and 255"
        )))
    })
}

fn u64_to_int(value: u64) -> RhaiResult<INT> {
    INT::try_from(value).map_err(|_| {
        to_rhai_error(HxError::CommandError(format!(
            "script integer overflow for value {value}"
        )))
    })
}

fn to_rhai_error(err: HxError) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(Dynamic::from(err.to_string()), Position::NONE).into()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::config::Config;

    fn file_document(path: &Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    #[test]
    fn extended_script_api_covers_read_hash_search_export_replace_and_save_as() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        let script = dir.path().join("patch.hxscript");
        let document = file_document(&file, b"abcXYZabcXYZzz");
        let source = r#"
if hx_len_display() != 14 || hx_len_logical() != 14 {
    throw "bad initial length";
}

hx_select_logical(3, 3);
if !hx_has_selection() {
    throw "missing selection";
}
if hx_selection_start() != 3 || hx_selection_len() != 3 || hx_selection_space() != "logical" {
    throw "bad selection metadata";
}
if hx_read_selection() != hx_ascii("XYZ") {
    throw "bad selection read";
}
if hx_read_display(3, 3) != hx_read_logical(3, 3) {
    throw "display/logical read mismatch";
}
if hx_hash_selection_hex("crc32") != hx_hash_display_hex(3, 3, "crc32") {
    throw "selection/display hash mismatch";
}
if hx_hash_selection_hex("crc32") != hx_hash_logical_hex(3, 3, "crc32") {
    throw "selection/logical hash mismatch";
}
if hx_hash_all_hex("crc32") == "" {
    throw "missing whole-file hash";
}

hx_export_selection("selection.bin");
hx_export_display(0, 3, "display.bin");
hx_export_logical(3, 3, "logical.bin");

hx_goto(0);
if hx_search_forward(hx_ascii("XYZ")) != 3 {
    throw "forward search failed";
}
if hx_search_forward_select(hx_ascii("XYZ")) != 3 {
    throw "forward select search failed";
}
hx_clear_selection();
hx_goto_end();
if hx_cursor() != hx_len_display() - 1 {
    throw "goto end failed";
}
if hx_search_backward(hx_ascii("abc")) != 6 {
    throw "backward search failed";
}
hx_goto_end();
if hx_search_backward_select(hx_ascii("abc")) != 6 {
    throw "backward select search failed";
}

hx_xor_selection(255);
hx_delete_selection();
hx_clear_selection();

hx_replace_all(hx_ascii("XYZ"), hx_ascii("uvw"), false, false);
if hx_read_logical(0, hx_len_logical()) != hx_ascii("abcuvwuvwzz") {
    throw "replace all failed";
}
hx_replace_display(0, 3, hx_ascii("abc"), hx_ascii("ABC"), false, false);
hx_xor_display(0, 3, 32);
hx_replace_logical(3, 3, hx_ascii("uvw"), hx_ascii("XYZ"), false, false);
hx_xor_logical(3, 3, 32);
hx_select_display(9, 3);
hx_replace_selection(hx_ascii("uvw"), hx_ascii("123"), false, false);
hx_delete_real_display(hx_len_display() - 1, 1);
hx_save_as("saved.bin");
"#;
        fs::write(&script, source).unwrap();

        let result = match run_script_source(&script, source, document, ExecState::new(0, None)) {
            Ok(result) => result,
            Err(failure) => panic!("script failed: {}", failure.error),
        };

        assert!(result.report.saved);
        assert_eq!(result.state.selection, None);
        assert_eq!(fs::read(dir.path().join("selection.bin")).unwrap(), b"XYZ");
        assert_eq!(fs::read(dir.path().join("display.bin")).unwrap(), b"abc");
        assert_eq!(fs::read(dir.path().join("logical.bin")).unwrap(), b"XYZ");
        assert_eq!(
            fs::read(dir.path().join("saved.bin")).unwrap(),
            b"abcxyz123z"
        );
        assert_eq!(fs::read(&file).unwrap(), b"abcXYZabcXYZzz");
    }

    #[test]
    fn extended_script_api_preserves_delete_semantics() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("delete.bin");
        let script = dir.path().join("delete.hxscript");
        let document = file_document(&file, b"abcd");
        let source = r#"
hx_delete_display(1, 1);
hx_select_display(2, 1);
hx_delete_selection();
hx_clear_selection();
hx_select_logical(1, 1);
hx_delete_logical(1, 1);
if hx_has_selection() {
    throw "logical selection should be cleared after tombstone delete";
}
if hx_len_display() != 4 || hx_len_logical() != 1 {
    throw "bad tombstone lengths";
}

hx_insert(hx_len_display(), hx_ascii("efgh"));
hx_delete_real_logical(1, 2);
hx_delete_real_display(hx_len_display() - 1, 1);
hx_insert(hx_len_display(), hx_ascii("i"));
hx_select_display(hx_len_display() - 1, 1);
hx_delete_real_selection();
hx_save();
"#;
        fs::write(&script, source).unwrap();

        let result = match run_script_source(&script, source, document, ExecState::new(0, None)) {
            Ok(result) => result,
            Err(failure) => panic!("script failed: {}", failure.error),
        };

        assert!(result.report.saved);
        assert_eq!(result.state.selection, None);
        assert_eq!(fs::read(&file).unwrap(), b"ag");
    }

    #[test]
    fn extended_script_api_errors_without_required_selection() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        let script = dir.path().join("missing_selection.hxscript");
        let document = file_document(&file, b"abcd");
        let source = r#"hx_hash_selection_hex("crc32");"#;
        fs::write(&script, source).unwrap();

        let failure = match run_script_source(&script, source, document, ExecState::new(0, None)) {
            Ok(_) => panic!("script unexpectedly succeeded"),
            Err(failure) => failure,
        };

        assert!(failure.error.to_string().contains("selection"));
    }
}
