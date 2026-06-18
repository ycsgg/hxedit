use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use rhai::{Blob, Dynamic, Engine, EvalAltResult, Position, INT};

use crate::commands::types::HashAlgorithm;
use crate::core::document::Document;
use crate::error::{HxError, HxResult};
use crate::exec::{
    execute_batch, ExecArtifact, ExecBatchOptions, ExecCommand, ExecOffset, ExecRange, ExecScope,
    ExecSearchDirection, ExecState, SearchSelect,
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
) -> Result<ScriptRunResult, ScriptRunFailure> {
    let budget = ScriptBudget::default();
    let host = Rc::new(RefCell::new(ScriptHost::new(document, state, budget)));
    let mut engine = Engine::new();
    engine.set_max_operations(budget.max_operations);
    register_api(&mut engine, Rc::clone(&host));

    let eval_result = engine.eval::<()>(source);

    drop(engine);
    let host = Rc::try_unwrap(host)
        .map_err(|_| ScriptRunFailure {
            document: Document::from_memory_bytes(
                "<script-host-borrowed>".into(),
                Vec::new(),
                &crate::config::Config::default(),
            ),
            state: ExecState::new(0, None),
            report: ScriptReport::default(),
            error: HxError::CommandError("script host is still borrowed".to_owned()),
        })?
        .into_inner();
    let result = host.into_result();

    match eval_result {
        Ok(()) => Ok(result),
        Err(err) => Err(ScriptRunFailure {
            document: result.document,
            state: result.state,
            report: result.report,
            error: HxError::CommandError(format!("script {}: {err}", path.display())),
        }),
    }
}

struct ScriptHost {
    document: Document,
    state: ExecState,
    budget: ScriptBudget,
    report: ScriptReport,
}

impl ScriptHost {
    fn new(document: Document, state: ExecState, budget: ScriptBudget) -> Self {
        Self {
            document,
            state,
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

    let goto_host = Rc::clone(&host);
    engine.register_fn("hx_goto", move |offset: INT| -> RhaiResult<()> {
        let offset = int_to_u64(offset, "offset")?;
        goto_host
            .borrow_mut()
            .execute(ExecCommand::Goto {
                target: ExecOffset::Absolute(offset),
            })
            .map(|_| ())
            .map_err(to_rhai_error)
    });

    let select_host = Rc::clone(&host);
    engine.register_fn(
        "hx_select_display",
        move |start: INT, len: INT| -> RhaiResult<()> {
            let start = int_to_u64(start, "selection start")?;
            let len = int_to_u64(len, "selection length")?;
            select_host
                .borrow_mut()
                .execute(ExecCommand::Select {
                    range: ExecRange::display(start, len),
                })
                .map(|_| ())
                .map_err(to_rhai_error)
        },
    );

    let clear_selection_host = Rc::clone(&host);
    engine.register_fn("hx_clear_selection", move || -> RhaiResult<()> {
        clear_selection_host
            .borrow_mut()
            .execute(ExecCommand::ClearSelection)
            .map(|_| ())
            .map_err(to_rhai_error)
    });

    let read_host = Rc::clone(&host);
    engine.register_fn(
        "hx_read_display",
        move |start: INT, len: INT| -> RhaiResult<Blob> {
            let start = int_to_u64(start, "read start")?;
            let len = int_to_u64(len, "read length")?;
            let mut host = read_host.borrow_mut();
            host.check_read_request(len).map_err(to_rhai_error)?;
            let outcome = host
                .execute(ExecCommand::Read {
                    scope: ExecScope::Range(ExecRange::display(start, len)),
                })
                .map_err(to_rhai_error)?;
            let bytes = outcome_bytes(outcome)?;
            host.record_read(bytes.len()).map_err(to_rhai_error)?;
            Ok(bytes)
        },
    );

    let read_selection_host = Rc::clone(&host);
    engine.register_fn("hx_read_selection", move || -> RhaiResult<Blob> {
        let mut host = read_selection_host.borrow_mut();
        if let Some(selection) = host.state.selection {
            host.check_read_request(selection.range.len)
                .map_err(to_rhai_error)?;
        }
        let outcome = host
            .execute(ExecCommand::Read {
                scope: ExecScope::Selection,
            })
            .map_err(to_rhai_error)?;
        let bytes = outcome_bytes(outcome)?;
        host.record_read(bytes.len()).map_err(to_rhai_error)?;
        Ok(bytes)
    });

    let search_host = Rc::clone(&host);
    engine.register_fn("hx_search", move |pattern: Blob| -> RhaiResult<INT> {
        search_host
            .borrow()
            .check_blob_len(pattern.len(), "search pattern")
            .map_err(to_rhai_error)?;
        let outcome = search_host
            .borrow_mut()
            .execute(ExecCommand::Search {
                pattern,
                direction: ExecSearchDirection::Forward,
                select: SearchSelect::None,
            })
            .map_err(to_rhai_error)?;
        match outcome.cursor {
            Some(offset) => u64_to_int(offset),
            None => Ok(-1),
        }
    });

    let hash_host = Rc::clone(&host);
    engine.register_fn(
        "hx_hash_hex",
        move |algorithm: &str| -> RhaiResult<String> {
            let algorithm = HashAlgorithm::parse(algorithm)
                .ok_or_else(|| HxError::InvalidHashAlgorithm(algorithm.to_owned()))
                .map_err(to_rhai_error)?;
            let scope = active_scope_or_all(&hash_host.borrow().state);
            let outcome = hash_host
                .borrow_mut()
                .execute(ExecCommand::Hash { algorithm, scope })
                .map_err(to_rhai_error)?;
            outcome_text(outcome)
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
                .execute(ExecCommand::Overwrite {
                    offset: ExecOffset::Absolute(offset),
                    bytes,
                })
                .map(|_| ())
                .map_err(to_rhai_error)
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
            insert_host
                .borrow_mut()
                .execute(ExecCommand::Insert {
                    offset: ExecOffset::Absolute(offset),
                    bytes,
                })
                .map(|_| ())
                .map_err(to_rhai_error)
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
            fill_host
                .borrow_mut()
                .execute(ExecCommand::Fill {
                    offset: ExecOffset::Absolute(offset),
                    pattern,
                    len,
                })
                .map(|_| ())
                .map_err(to_rhai_error)
        },
    );

    let save_host = Rc::clone(&host);
    engine.register_fn("hx_save", move || -> RhaiResult<()> {
        save_host
            .borrow_mut()
            .execute(ExecCommand::Save { path: None })
            .map(|_| ())
            .map_err(to_rhai_error)
    });
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
