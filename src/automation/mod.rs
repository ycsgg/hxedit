use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::commands::types::HashAlgorithm;
use crate::core::document::Document;
use crate::error::{HxError, HxResult};
use crate::exec::{
    execute_batch, undo_edit_op, DeleteKind, EditOp, ExecArtifact, ExecBatchOptions,
    ExecBatchReport, ExecCommand, ExecErrorPolicy, ExecOffset, ExecRange, ExecScope,
    ExecSearchDirection, ExecState, ExecStep, ExecUndoPolicy, RangeSpace, SearchSelect,
};
use crate::util::parse::{parse_hex_stream, parse_offset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroSelectionPolicy {
    Inherit,
    Clear,
    Require,
}

#[derive(Debug, Clone)]
pub(crate) struct MacroProgram {
    selection: MacroSelectionPolicy,
    options: ExecBatchOptions,
    steps: Vec<MacroStep>,
}

impl MacroProgram {
    pub(crate) fn load(path: &Path) -> HxResult<Self> {
        let source = std::fs::read_to_string(path)?;
        let raw: RawMacro = toml::from_str(&source).map_err(|err| {
            HxError::CommandError(format!("invalid macro file {}: {err}", path.display()))
        })?;
        raw.into_program(path.parent().unwrap_or_else(|| Path::new(".")))
    }

    pub(crate) fn execute(
        &self,
        document: &mut Document,
        state: &mut ExecState,
    ) -> HxResult<ExecBatchReport> {
        match self.selection {
            MacroSelectionPolicy::Inherit => {}
            MacroSelectionPolicy::Clear => state.selection = None,
            MacroSelectionPolicy::Require if state.selection.is_none() => {
                return Err(HxError::MissingSelection);
            }
            MacroSelectionPolicy::Require => {}
        }

        self.execute_steps(document, state)
    }

    fn execute_steps(
        &self,
        document: &mut Document,
        state: &mut ExecState,
    ) -> HxResult<ExecBatchReport> {
        let initial_state = *state;
        let mut context = MacroContext::default();
        let mut report = ExecBatchReport::default();
        let mut group_start = initial_state;
        let mut group_ops = Vec::new();
        let mut applied_ops = Vec::new();

        for step in &self.steps {
            let step_before = *state;
            let command = match step.to_command(&context) {
                Ok(command) => command,
                Err(err) => {
                    handle_macro_error(
                        document,
                        state,
                        initial_state,
                        self.options,
                        MacroErrorState {
                            report: &mut report,
                            applied_ops: &mut applied_ops,
                            group_ops: &mut group_ops,
                        },
                        err,
                    )?;
                    break;
                }
            };

            if self.options.on_error == ExecErrorPolicy::Rollback
                && matches!(
                    command,
                    ExecCommand::Save { .. } | ExecCommand::ExportBinary { .. }
                )
            {
                handle_macro_error(
                    document,
                    state,
                    initial_state,
                    self.options,
                    MacroErrorState {
                        report: &mut report,
                        applied_ops: &mut applied_ops,
                        group_ops: &mut group_ops,
                    },
                    HxError::CommandError(
                        "rollback macros cannot contain save or export-binary steps".to_owned(),
                    ),
                )?;
                break;
            }

            let mut single_report = execute_batch(
                document,
                state,
                std::slice::from_ref(&command),
                ExecBatchOptions {
                    undo: ExecUndoPolicy::PerStep,
                    on_error: ExecErrorPolicy::Stop,
                },
            )?;

            if let Some(error) = single_report.error.take() {
                handle_macro_error(
                    document,
                    state,
                    initial_state,
                    self.options,
                    MacroErrorState {
                        report: &mut report,
                        applied_ops: &mut applied_ops,
                        group_ops: &mut group_ops,
                    },
                    HxError::CommandError(error),
                )?;
                break;
            }

            if single_report.saved {
                group_ops.clear();
                applied_ops.clear();
                report.undo_steps.clear();
                report.saved = true;
            }

            for undo_step in single_report.undo_steps {
                if !undo_step.ops.is_empty() {
                    applied_ops.extend(undo_step.ops.iter().cloned());
                    match self.options.undo {
                        ExecUndoPolicy::Group => {
                            if group_ops.is_empty() {
                                group_start = step_before;
                            }
                            group_ops.extend(undo_step.ops);
                        }
                        ExecUndoPolicy::PerStep => report.undo_steps.push(undo_step),
                    }
                }
            }

            report.steps_completed += single_report.steps_completed;
            let bind_result = single_report
                .outcomes
                .last()
                .map(|outcome| step.bind_outcome(&mut context, &command, outcome))
                .transpose();
            report.outcomes.extend(single_report.outcomes);

            if let Err(err) = bind_result {
                handle_macro_error(
                    document,
                    state,
                    initial_state,
                    self.options,
                    MacroErrorState {
                        report: &mut report,
                        applied_ops: &mut applied_ops,
                        group_ops: &mut group_ops,
                    },
                    err,
                )?;
                break;
            }
        }

        if self.options.undo == ExecUndoPolicy::Group && !group_ops.is_empty() {
            report.undo_steps.push(ExecStep {
                cursor_before: group_start.cursor,
                selection_before: group_start.selection,
                cursor_after: state.cursor,
                selection_after: state.selection,
                ops: group_ops,
            });
        }

        Ok(report)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMacro {
    version: u32,
    #[serde(default = "default_selection")]
    selection: RawSelectionPolicy,
    #[serde(default = "default_undo")]
    undo: RawUndoPolicy,
    #[serde(default = "default_on_error")]
    on_error: RawErrorPolicy,
    steps: Vec<RawStep>,
}

impl RawMacro {
    fn into_program(self, base_dir: &Path) -> HxResult<MacroProgram> {
        if self.version != 1 {
            return Err(HxError::CommandError(format!(
                "unsupported macro version {}; expected 1",
                self.version
            )));
        }

        let selection = self.selection.into();
        let options = ExecBatchOptions {
            undo: self.undo.into(),
            on_error: self.on_error.into(),
        };
        let steps = self
            .steps
            .into_iter()
            .map(|step| step.into_step(base_dir))
            .collect::<HxResult<Vec<_>>>()?;
        Ok(MacroProgram {
            selection,
            options,
            steps,
        })
    }
}

struct MacroErrorState<'a> {
    report: &'a mut ExecBatchReport,
    applied_ops: &'a mut Vec<EditOp>,
    group_ops: &'a mut Vec<EditOp>,
}

fn handle_macro_error(
    document: &mut Document,
    state: &mut ExecState,
    initial_state: ExecState,
    options: ExecBatchOptions,
    error_state: MacroErrorState<'_>,
    err: HxError,
) -> HxResult<()> {
    if options.on_error == ExecErrorPolicy::Rollback {
        for op in error_state.applied_ops.iter().rev() {
            undo_edit_op(document, op)?;
        }
        *state = initial_state;
        error_state.group_ops.clear();
        error_state.applied_ops.clear();
        error_state.report.undo_steps.clear();
        error_state.report.outcomes.clear();
    }
    error_state.report.error = Some(err.to_string());
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawSelectionPolicy {
    Inherit,
    Clear,
    Require,
}

impl From<RawSelectionPolicy> for MacroSelectionPolicy {
    fn from(value: RawSelectionPolicy) -> Self {
        match value {
            RawSelectionPolicy::Inherit => Self::Inherit,
            RawSelectionPolicy::Clear => Self::Clear,
            RawSelectionPolicy::Require => Self::Require,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawUndoPolicy {
    Group,
    PerStep,
}

impl From<RawUndoPolicy> for ExecUndoPolicy {
    fn from(value: RawUndoPolicy) -> Self {
        match value {
            RawUndoPolicy::Group => Self::Group,
            RawUndoPolicy::PerStep => Self::PerStep,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawErrorPolicy {
    Stop,
    Rollback,
}

impl From<RawErrorPolicy> for ExecErrorPolicy {
    fn from(value: RawErrorPolicy) -> Self {
        match value {
            RawErrorPolicy::Stop => Self::Stop,
            RawErrorPolicy::Rollback => Self::Rollback,
        }
    }
}

fn default_selection() -> RawSelectionPolicy {
    RawSelectionPolicy::Inherit
}

fn default_undo() -> RawUndoPolicy {
    RawUndoPolicy::Group
}

fn default_on_error() -> RawErrorPolicy {
    RawErrorPolicy::Stop
}

#[derive(Debug, Clone)]
struct MacroStep {
    id: Option<String>,
    command: MacroCommand,
}

impl MacroStep {
    fn new(id: Option<String>, command: MacroCommand) -> HxResult<Self> {
        if let Some(id) = &id {
            validate_binding_id(id)?;
        }
        Ok(Self { id, command })
    }

    fn to_command(&self, context: &MacroContext) -> HxResult<ExecCommand> {
        self.command.to_command(context)
    }

    fn bind_outcome(
        &self,
        context: &mut MacroContext,
        command: &ExecCommand,
        outcome: &crate::exec::ExecOutcome,
    ) -> HxResult<()> {
        let Some(id) = &self.id else {
            return Ok(());
        };

        let value = match (&self.command, command) {
            (MacroCommand::Read { .. }, _) => {
                let bytes = outcome
                    .artifacts
                    .iter()
                    .find_map(|artifact| match artifact {
                        ExecArtifact::Bytes(bytes) => Some(bytes.clone()),
                        ExecArtifact::Text(_) | ExecArtifact::File(_) => None,
                    })
                    .unwrap_or_default();
                MacroValue::from_bytes(bytes)
            }
            (MacroCommand::Hash { .. }, _) => {
                let hex = outcome
                    .artifacts
                    .iter()
                    .find_map(|artifact| match artifact {
                        ExecArtifact::Text(text) => Some(text.clone()),
                        ExecArtifact::Bytes(_) | ExecArtifact::File(_) => None,
                    })
                    .unwrap_or_default();
                MacroValue::from_hash_hex(hex)?
            }
            (MacroCommand::Search { .. }, ExecCommand::Search { pattern, .. }) => {
                if outcome.cursor.is_some() {
                    MacroValue::from_bytes(pattern.clone())
                } else {
                    MacroValue::from_bytes(Vec::new())
                }
            }
            _ => return Ok(()),
        };
        context.values.insert(id.clone(), value);
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum MacroCommand {
    Goto {
        target: ExecOffset,
    },
    Select {
        range: ExecRange,
    },
    ClearSelection,
    Read {
        scope: ExecScope,
    },
    Hash {
        algorithm: HashAlgorithm,
        scope: ExecScope,
    },
    Search {
        pattern: MacroBytes,
        direction: ExecSearchDirection,
        select: SearchSelect,
    },
    Overwrite {
        offset: ExecOffset,
        bytes: MacroBytes,
    },
    Insert {
        offset: ExecOffset,
        bytes: MacroBytes,
    },
    Delete {
        scope: ExecScope,
        kind: DeleteKind,
    },
    Fill {
        offset: ExecOffset,
        pattern: MacroBytes,
        len: u64,
    },
    XorInPlace {
        scope: ExecScope,
        key: u8,
    },
    Replace {
        scope: ExecScope,
        needle: MacroBytes,
        replacement: MacroBytes,
        allow_resize: bool,
        force: bool,
    },
    ExportBinary {
        scope: ExecScope,
        path: PathBuf,
    },
    Save {
        path: Option<PathBuf>,
    },
}

impl MacroCommand {
    fn to_command(&self, context: &MacroContext) -> HxResult<ExecCommand> {
        match self {
            MacroCommand::Goto { target } => Ok(ExecCommand::Goto { target: *target }),
            MacroCommand::Select { range } => Ok(ExecCommand::Select { range: *range }),
            MacroCommand::ClearSelection => Ok(ExecCommand::ClearSelection),
            MacroCommand::Read { scope } => Ok(ExecCommand::Read { scope: *scope }),
            MacroCommand::Hash { algorithm, scope } => Ok(ExecCommand::Hash {
                algorithm: *algorithm,
                scope: *scope,
            }),
            MacroCommand::Search {
                pattern,
                direction,
                select,
            } => Ok(ExecCommand::Search {
                pattern: pattern.resolve(context)?,
                direction: *direction,
                select: *select,
            }),
            MacroCommand::Overwrite { offset, bytes } => Ok(ExecCommand::Overwrite {
                offset: *offset,
                bytes: bytes.resolve(context)?,
            }),
            MacroCommand::Insert { offset, bytes } => Ok(ExecCommand::Insert {
                offset: *offset,
                bytes: bytes.resolve(context)?,
            }),
            MacroCommand::Delete { scope, kind } => Ok(ExecCommand::Delete {
                scope: *scope,
                kind: *kind,
            }),
            MacroCommand::Fill {
                offset,
                pattern,
                len,
            } => Ok(ExecCommand::Fill {
                offset: *offset,
                pattern: pattern.resolve(context)?,
                len: *len,
            }),
            MacroCommand::XorInPlace { scope, key } => Ok(ExecCommand::XorInPlace {
                scope: *scope,
                key: *key,
            }),
            MacroCommand::Replace {
                scope,
                needle,
                replacement,
                allow_resize,
                force,
            } => Ok(ExecCommand::Replace {
                scope: *scope,
                needle: needle.resolve(context)?,
                replacement: replacement.resolve(context)?,
                allow_resize: *allow_resize,
                force: *force,
            }),
            MacroCommand::ExportBinary { scope, path } => Ok(ExecCommand::ExportBinary {
                scope: *scope,
                path: path.clone(),
            }),
            MacroCommand::Save { path } => Ok(ExecCommand::Save { path: path.clone() }),
        }
    }
}

#[derive(Debug, Clone)]
enum MacroBytes {
    Literal(Vec<u8>),
    From {
        name: String,
        format: MacroValueFormat,
    },
}

impl MacroBytes {
    fn resolve(&self, context: &MacroContext) -> HxResult<Vec<u8>> {
        match self {
            MacroBytes::Literal(bytes) => Ok(bytes.clone()),
            MacroBytes::From { name, format } => {
                let value = context.values.get(name).ok_or_else(|| {
                    HxError::CommandError(format!("unknown macro variable: {name}"))
                })?;
                Ok(match format {
                    MacroValueFormat::Bytes => value.bytes.clone(),
                    MacroValueFormat::HexText => value.hex.as_bytes().to_vec(),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MacroValueFormat {
    Bytes,
    HexText,
}

#[derive(Debug, Clone)]
struct MacroValue {
    bytes: Vec<u8>,
    hex: String,
}

impl MacroValue {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        let hex = hex_string(&bytes);
        Self { bytes, hex }
    }

    fn from_hash_hex(hex: String) -> HxResult<Self> {
        let bytes = hex_string_to_bytes(&hex)?;
        Ok(Self { bytes, hex })
    }
}

#[derive(Debug, Default)]
struct MacroContext {
    values: BTreeMap<String, MacroValue>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
enum RawStep {
    Goto {
        offset: RawOffset,
    },
    Select {
        space: RawRangeSpace,
        start: RawNumber,
        len: RawNumber,
    },
    ClearSelection,
    Read {
        id: Option<String>,
        scope: RawScope,
    },
    Hash {
        id: Option<String>,
        algorithm: String,
        scope: RawScope,
    },
    Search {
        id: Option<String>,
        #[serde(default = "default_mode")]
        mode: RawPatternMode,
        pattern: RawBytes,
        #[serde(default = "default_direction")]
        direction: RawDirection,
        #[serde(default = "default_search_select")]
        select: RawSearchSelect,
    },
    Overwrite {
        #[serde(default = "default_offset")]
        offset: RawOffset,
        bytes: RawBytes,
    },
    Insert {
        #[serde(default = "default_offset")]
        offset: RawOffset,
        bytes: RawBytes,
    },
    Delete {
        scope: RawScope,
        #[serde(default = "default_delete_kind")]
        kind: RawDeleteKind,
    },
    Fill {
        #[serde(default = "default_offset")]
        offset: RawOffset,
        pattern: RawBytes,
        len: RawNumber,
    },
    Xor {
        scope: RawScope,
        key: RawNumber,
        #[serde(default = "default_true")]
        in_place: bool,
    },
    Replace {
        scope: RawScope,
        #[serde(default = "default_mode")]
        mode: RawPatternMode,
        needle: RawBytes,
        replacement: RawBytes,
        #[serde(default)]
        allow_resize: bool,
        #[serde(default)]
        force: bool,
    },
    ExportBinary {
        scope: RawScope,
        path: String,
    },
    Save {
        path: Option<String>,
    },
}

impl RawStep {
    fn into_step(self, base_dir: &Path) -> HxResult<MacroStep> {
        match self {
            RawStep::Goto { offset } => MacroStep::new(
                None,
                MacroCommand::Goto {
                    target: offset.parse()?,
                },
            ),
            RawStep::Select { space, start, len } => MacroStep::new(
                None,
                MacroCommand::Select {
                    range: ExecRange {
                        space: space.into(),
                        start: start.parse_u64("select start")?,
                        len: len.parse_u64("select len")?,
                    },
                },
            ),
            RawStep::ClearSelection => MacroStep::new(None, MacroCommand::ClearSelection),
            RawStep::Read { id, scope } => MacroStep::new(
                id,
                MacroCommand::Read {
                    scope: scope.parse()?,
                },
            ),
            RawStep::Hash {
                id,
                algorithm,
                scope,
            } => MacroStep::new(
                id,
                MacroCommand::Hash {
                    algorithm: parse_hash_algorithm(&algorithm)?,
                    scope: scope.parse()?,
                },
            ),
            RawStep::Search {
                id,
                mode,
                pattern,
                direction,
                select,
            } => MacroStep::new(
                id,
                MacroCommand::Search {
                    pattern: pattern.into_macro_bytes(mode)?,
                    direction: direction.into(),
                    select: select.into(),
                },
            ),
            RawStep::Overwrite { offset, bytes } => MacroStep::new(
                None,
                MacroCommand::Overwrite {
                    offset: offset.parse()?,
                    bytes: bytes.into_hex_macro_bytes()?,
                },
            ),
            RawStep::Insert { offset, bytes } => MacroStep::new(
                None,
                MacroCommand::Insert {
                    offset: offset.parse()?,
                    bytes: bytes.into_hex_macro_bytes()?,
                },
            ),
            RawStep::Delete { scope, kind } => MacroStep::new(
                None,
                MacroCommand::Delete {
                    scope: scope.parse()?,
                    kind: kind.into(),
                },
            ),
            RawStep::Fill {
                offset,
                pattern,
                len,
            } => MacroStep::new(
                None,
                MacroCommand::Fill {
                    offset: offset.parse()?,
                    pattern: pattern.into_hex_macro_bytes()?,
                    len: len.parse_u64("fill len")?,
                },
            ),
            RawStep::Xor {
                scope,
                key,
                in_place,
            } => {
                if !in_place {
                    return Err(HxError::CommandError(
                        "macro xor requires in_place = true".to_owned(),
                    ));
                }
                MacroStep::new(
                    None,
                    MacroCommand::XorInPlace {
                        scope: scope.parse()?,
                        key: parse_u8_key(key)?,
                    },
                )
            }
            RawStep::Replace {
                scope,
                mode,
                needle,
                replacement,
                allow_resize,
                force,
            } => MacroStep::new(
                None,
                MacroCommand::Replace {
                    scope: scope.parse()?,
                    needle: needle.into_macro_bytes(mode)?,
                    replacement: replacement.into_macro_bytes(mode)?,
                    allow_resize,
                    force,
                },
            ),
            RawStep::ExportBinary { scope, path } => MacroStep::new(
                None,
                MacroCommand::ExportBinary {
                    scope: scope.parse()?,
                    path: resolve_macro_path(base_dir, &path),
                },
            ),
            RawStep::Save { path } => MacroStep::new(
                None,
                MacroCommand::Save {
                    path: path.map(|path| resolve_macro_path(base_dir, &path)),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawRangeSpace {
    Display,
    Logical,
}

impl From<RawRangeSpace> for RangeSpace {
    fn from(value: RawRangeSpace) -> Self {
        match value {
            RawRangeSpace::Display => Self::Display,
            RawRangeSpace::Logical => Self::Logical,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawNumber {
    Integer(i64),
    String(String),
}

impl RawNumber {
    fn parse_u64(&self, label: &'static str) -> HxResult<u64> {
        match self {
            RawNumber::Integer(value) => u64::try_from(*value).map_err(|_| {
                HxError::CommandError(format!("{label} must be a non-negative integer"))
            }),
            RawNumber::String(value) => parse_offset(value),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawOffset {
    Integer(i64),
    String(String),
}

impl RawOffset {
    fn parse(&self) -> HxResult<ExecOffset> {
        match self {
            RawOffset::Integer(value) => {
                let value =
                    u64::try_from(*value).map_err(|_| HxError::InvalidOffset(value.to_string()))?;
                Ok(ExecOffset::Absolute(value))
            }
            RawOffset::String(value) => parse_offset_expr(value),
        }
    }
}

fn default_offset() -> RawOffset {
    RawOffset::String("cursor".to_owned())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawScope {
    Named(String),
    Range {
        space: RawRangeSpace,
        start: RawNumber,
        len: RawNumber,
    },
}

impl RawScope {
    fn parse(self) -> HxResult<ExecScope> {
        match self {
            RawScope::Named(name) => match name.as_str() {
                "selection" => Ok(ExecScope::Selection),
                "all" => Ok(ExecScope::All),
                other => Err(HxError::CommandError(format!(
                    "unknown macro scope: {other}"
                ))),
            },
            RawScope::Range { space, start, len } => Ok(ExecScope::Range(ExecRange {
                space: space.into(),
                start: start.parse_u64("range start")?,
                len: len.parse_u64("range len")?,
            })),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawPatternMode {
    Hex,
    X,
    Text,
    Ascii,
    Utf8,
    Byte,
    B,
}

impl RawPatternMode {
    fn parse_bytes(self, value: &str) -> HxResult<Vec<u8>> {
        match self {
            RawPatternMode::Hex | RawPatternMode::X => parse_hex_stream(value),
            RawPatternMode::Text | RawPatternMode::Ascii | RawPatternMode::Utf8 => {
                Ok(value.as_bytes().to_vec())
            }
            RawPatternMode::Byte | RawPatternMode::B => {
                let value = parse_offset(value)?;
                let byte =
                    u8::try_from(value).map_err(|_| HxError::InvalidOffset(value.to_string()))?;
                Ok(vec![byte])
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawBytes {
    Literal(String),
    From {
        from: String,
        #[serde(default)]
        format: RawValueFormat,
    },
}

impl RawBytes {
    fn into_hex_macro_bytes(self) -> HxResult<MacroBytes> {
        self.into_macro_bytes(RawPatternMode::Hex)
    }

    fn into_macro_bytes(self, mode: RawPatternMode) -> HxResult<MacroBytes> {
        match self {
            RawBytes::Literal(value) => Ok(MacroBytes::Literal(mode.parse_bytes(&value)?)),
            RawBytes::From { from, format } => {
                validate_binding_id(&from)?;
                Ok(MacroBytes::From {
                    name: from,
                    format: format.into(),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum RawValueFormat {
    #[default]
    Bytes,
    HexText,
}

impl From<RawValueFormat> for MacroValueFormat {
    fn from(value: RawValueFormat) -> Self {
        match value {
            RawValueFormat::Bytes => Self::Bytes,
            RawValueFormat::HexText => Self::HexText,
        }
    }
}

fn default_mode() -> RawPatternMode {
    RawPatternMode::Hex
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawDirection {
    Forward,
    Backward,
}

impl From<RawDirection> for ExecSearchDirection {
    fn from(value: RawDirection) -> Self {
        match value {
            RawDirection::Forward => Self::Forward,
            RawDirection::Backward => Self::Backward,
        }
    }
}

fn default_direction() -> RawDirection {
    RawDirection::Forward
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawSearchSelect {
    None,
    Match,
}

impl From<RawSearchSelect> for SearchSelect {
    fn from(value: RawSearchSelect) -> Self {
        match value {
            RawSearchSelect::None => Self::None,
            RawSearchSelect::Match => Self::Match,
        }
    }
}

fn default_search_select() -> RawSearchSelect {
    RawSearchSelect::None
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawDeleteKind {
    Tombstone,
    Real,
}

impl From<RawDeleteKind> for DeleteKind {
    fn from(value: RawDeleteKind) -> Self {
        match value {
            RawDeleteKind::Tombstone => Self::Tombstone,
            RawDeleteKind::Real => Self::Real,
        }
    }
}

fn default_delete_kind() -> RawDeleteKind {
    RawDeleteKind::Tombstone
}

fn default_true() -> bool {
    true
}

fn parse_hash_algorithm(input: &str) -> HxResult<HashAlgorithm> {
    HashAlgorithm::parse(input).ok_or_else(|| HxError::InvalidHashAlgorithm(input.to_owned()))
}

fn parse_u8_key(input: RawNumber) -> HxResult<u8> {
    let value = input.parse_u64("xor key")?;
    u8::try_from(value).map_err(|_| HxError::InvalidXorKey(value.to_string()))
}

fn parse_offset_expr(input: &str) -> HxResult<ExecOffset> {
    let value = input.trim();
    if value == "cursor" {
        return Ok(ExecOffset::Cursor(0));
    }
    if value == "end" {
        return Ok(ExecOffset::End);
    }
    if let Some(delta) = value.strip_prefix("cursor+") {
        let delta = parse_offset(delta)?;
        let delta = i64::try_from(delta).map_err(|_| HxError::InvalidOffset(input.to_owned()))?;
        return Ok(ExecOffset::Cursor(delta));
    }
    if let Some(delta) = value.strip_prefix("cursor-") {
        let delta = parse_offset(delta)?;
        let delta = i64::try_from(delta).map_err(|_| HxError::InvalidOffset(input.to_owned()))?;
        return Ok(ExecOffset::Cursor(-delta));
    }
    Ok(ExecOffset::Absolute(parse_offset(value)?))
}

fn resolve_macro_path(base_dir: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn validate_binding_id(id: &str) -> HxResult<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(HxError::CommandError(format!(
            "invalid macro variable name: {id}"
        )));
    }
    Ok(())
}

fn hex_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn hex_string_to_bytes(hex: &str) -> HxResult<Vec<u8>> {
    if hex.is_empty() {
        return Ok(Vec::new());
    }
    if !hex.len().is_multiple_of(2) {
        return Err(HxError::InvalidHexPattern(hex.to_owned()));
    }

    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let hi = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| HxError::InvalidHexPattern(hex.to_owned()))?;
        let lo = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| HxError::InvalidHexPattern(hex.to_owned()))?;
        out.push(((hi << 4) | lo) as u8);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn document_with_bytes(bytes: &[u8]) -> Document {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, bytes).unwrap();
        Document::open(file.path(), &Config::default()).unwrap()
    }

    fn load_from_str(source: &str) -> HxResult<MacroProgram> {
        let raw: RawMacro = toml::from_str(source)
            .map_err(|err| HxError::CommandError(format!("invalid macro: {err}")))?;
        raw.into_program(Path::new("/tmp/macros"))
    }

    #[test]
    fn parses_basic_macro_file() {
        let program = load_from_str(
            r#"
version = 1
selection = "inherit"
undo = "group"
on_error = "stop"

[[steps]]
cmd = "select"
space = "display"
start = "0x10"
len = 4

[[steps]]
cmd = "xor"
scope = "selection"
key = "0xaa"
in_place = true
"#,
        )
        .unwrap();

        assert_eq!(program.selection, MacroSelectionPolicy::Inherit);
        assert_eq!(program.options.undo, ExecUndoPolicy::Group);
        assert_eq!(program.steps.len(), 2);
        assert!(matches!(
            program.steps[0].command,
            MacroCommand::Select {
                range: ExecRange {
                    space: RangeSpace::Display,
                    start: 0x10,
                    len: 4,
                }
            }
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = load_from_str(
            r#"
version = 1
unknown = true
steps = []
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn resolves_relative_paths_against_macro_file() {
        let program = load_from_str(
            r#"
version = 1

[[steps]]
cmd = "export-binary"
scope = "all"
path = "out.bin"
"#,
        )
        .unwrap();

        assert!(matches!(
            &program.steps[0].command,
            MacroCommand::ExportBinary { path, .. } if path == &PathBuf::from("/tmp/macros/out.bin")
        ));
    }

    #[test]
    fn hash_result_can_be_reused_as_bytes_and_hex_text() {
        let program = load_from_str(
            r#"
version = 1

[[steps]]
cmd = "hash"
id = "payload_crc32"
algorithm = "crc32"
scope = { space = "display", start = "0x0", len = 4 }

[[steps]]
cmd = "insert"
offset = "0x4"
bytes = { from = "payload_crc32", format = "bytes" }

[[steps]]
cmd = "insert"
offset = "0x8"
bytes = { from = "payload_crc32", format = "hex-text" }
"#,
        )
        .unwrap();
        let mut document = document_with_bytes(b"abcd");
        let mut state = ExecState::new(0, None);

        let report = program.execute(&mut document, &mut state).unwrap();

        assert_eq!(report.error, None);
        let digest = crc32fast::hash(b"abcd").to_be_bytes();
        let hex = hex_string(&digest);
        let mut expected = b"abcd".to_vec();
        expected.extend_from_slice(&digest);
        expected.extend_from_slice(hex.as_bytes());
        assert_eq!(
            document.logical_bytes(0, document.len() - 1).unwrap(),
            expected
        );
        assert_eq!(report.undo_steps.len(), 1);
    }

    #[test]
    fn read_and_search_results_can_be_reused_as_bytes() {
        let program = load_from_str(
            r#"
version = 1

[[steps]]
cmd = "read"
id = "prefix"
scope = { space = "display", start = "0x0", len = 2 }

[[steps]]
cmd = "insert"
offset = "0x4"
bytes = { from = "prefix" }

[[steps]]
cmd = "goto"
offset = "0x0"

[[steps]]
cmd = "search"
id = "hit"
pattern = { from = "prefix" }
direction = "forward"
select = "match"

[[steps]]
cmd = "overwrite"
offset = "0x2"
bytes = { from = "hit" }
"#,
        )
        .unwrap();
        let mut document = document_with_bytes(b"abcd");
        let mut state = ExecState::new(0, None);

        let report = program.execute(&mut document, &mut state).unwrap();

        assert_eq!(report.error, None);
        assert_eq!(
            document.logical_bytes(0, document.len() - 1).unwrap(),
            b"ababab"
        );
        assert_eq!(
            state.selection.map(|selection| selection.range),
            Some(ExecRange::display(0, 2))
        );
    }

    #[test]
    fn unknown_variable_rolls_back_when_requested() {
        let program = load_from_str(
            r#"
version = 1
on_error = "rollback"

[[steps]]
cmd = "overwrite"
offset = "0x1"
bytes = "aa"

[[steps]]
cmd = "insert"
offset = "0x2"
bytes = { from = "missing" }
"#,
        )
        .unwrap();
        let mut document = document_with_bytes(b"abcd");
        let mut state = ExecState::new(0, None);

        let report = program.execute(&mut document, &mut state).unwrap();

        assert!(report
            .error
            .as_deref()
            .is_some_and(|error| error.contains("unknown macro variable")));
        assert!(report.undo_steps.is_empty());
        assert_eq!(
            document.logical_bytes(0, document.len() - 1).unwrap(),
            b"abcd"
        );
    }
}
