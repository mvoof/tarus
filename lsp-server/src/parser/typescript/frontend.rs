//! TypeScript/JavaScript/Vue/Svelte/Angular parsing for Tauri invoke/emit/listen calls

use crate::indexer::{DynamicUsages, Finding, Forwarder};
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use crate::utils::{find_capture, point_to_position};
use std::collections::HashMap;
use std::sync::LazyLock;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::extractors::{count_specta_call_args, extract_type_argument_info};
use super::sfc::{adjust_position, adjust_range};
use crate::parser::lang_config::{get_query_source, LangType};

/// Function patterns with their argument position
struct FunctionPatternWithPos {
    name: &'static str,
    entity: EntityType,
    behavior: Behavior,
    arg_position: ArgPosition,
}

#[derive(Clone, Copy, PartialEq)]
enum ArgPosition {
    First,
    Second,
}

static ALL_FRONTEND_PATTERNS: LazyLock<Vec<FunctionPatternWithPos>> = LazyLock::new(|| {
    vec![
        FunctionPatternWithPos {
            name: "invoke",
            entity: EntityType::Command,
            behavior: Behavior::Call,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "emit",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "listen",
            entity: EntityType::Event,
            behavior: Behavior::Listen,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "once",
            entity: EntityType::Event,
            behavior: Behavior::Listen,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "emitTo",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::Second,
        },
    ]
});

/// Result of parsing one frontend file.
#[derive(Default)]
pub struct FrontendParse {
    pub findings: Vec<Finding>,
    pub dynamic_usages: DynamicUsages,
    /// Forwarding helpers this file declares.
    pub forwarders: Vec<Forwarder>,
}

/// Everything needed to turn a command/event name argument into a literal.
struct Scope<'a> {
    /// String constants declared in the file being parsed.
    local_constants: HashMap<String, String>,
    /// String constants declared anywhere else in the workspace.
    global_constants: &'a HashMap<String, String>,
    /// Identifiers the file binds itself; they shadow `global_constants`.
    bound_names: std::collections::HashSet<String>,
}

impl Scope<'_> {
    /// Resolve an identifier or member expression to the string it stands for.
    ///
    /// Returns `None` when the name cannot be proven — either it is bound locally
    /// to something that is not a string literal, or no constant declares it. A
    /// workspace-wide constant is never applied to a locally bound name: doing so
    /// invents a name out of an unrelated file and reports diagnostics against it.
    fn resolve(&self, raw: &str) -> Option<String> {
        let base = raw.split('.').next().unwrap_or(raw);
        let last = raw.split('.').next_back().unwrap_or(raw);

        if let Some(value) = self
            .local_constants
            .get(raw)
            .or_else(|| self.local_constants.get(last))
        {
            return Some(value.clone());
        }

        if self.bound_names.contains(base) {
            return None;
        }

        self.global_constants
            .get(raw)
            .or_else(|| self.global_constants.get(last))
            .cloned()
    }
}

/// What a query match yielded for one of the call patterns.
enum PatternOutcome {
    /// The match is a Tauri call with a known command/event name.
    Found(Finding),
    /// The match is a Tauri call that forwards a parameter of the enclosing helper.
    /// The name lives at the helper's call sites, not here.
    Forwards(Forwarder),
    /// The match is a Tauri call whose name could not be resolved to a literal.
    Dynamic(Behavior),
    /// The match is not a Tauri call at all.
    NoMatch,
}

/// Node kinds that introduce a parameter list.
const FUNCTION_KINDS: [&str; 5] = [
    "arrow_function",
    "function_declaration",
    "function_expression",
    "method_definition",
    "generator_function_declaration",
];

/// If `name` is a parameter of a *named* function enclosing `node`, describe how
/// that function forwards it into a Tauri call.
///
/// Anonymous callbacks are skipped: with no name there is no call site to look up.
fn detect_forwarder(
    node: tree_sitter::Node<'_>,
    name: &str,
    bytes: &[u8],
    entity: EntityType,
    behavior: Behavior,
) -> Option<Forwarder> {
    let mut current = node.parent();

    while let Some(candidate) = current {
        if FUNCTION_KINDS.contains(&candidate.kind()) {
            if let Some(param_index) = parameter_index(candidate, name, bytes) {
                return function_name(candidate, bytes).map(|function_name| Forwarder {
                    function_name,
                    param_index,
                    entity,
                    behavior,
                });
            }

            // The name belongs to an outer scope; keep walking outwards.
        }

        current = candidate.parent();
    }

    None
}

/// Position of the parameter called `name` in a function's parameter list.
fn parameter_index(function: tree_sitter::Node<'_>, name: &str, bytes: &[u8]) -> Option<usize> {
    // `const f = event => ...` has a single unparenthesized parameter.
    if let Some(single) = function.child_by_field_name("parameter") {
        return (single.utf8_text(bytes).unwrap_or_default() == name).then_some(0);
    }

    let params = function.child_by_field_name("parameters")?;
    let mut cursor = params.walk();
    let declared: Vec<_> = params.named_children(&mut cursor).collect();

    declared.iter().position(|param| {
        let mut bound = std::collections::HashSet::new();
        crate::utils::collect_parameter_names(*param, bytes, &mut bound);

        bound.contains(name)
    })
}

/// Name a function can be called by: its own identifier, or the variable it is
/// assigned to for arrow and function expressions.
fn function_name(function: tree_sitter::Node<'_>, bytes: &[u8]) -> Option<String> {
    if let Some(own_name) = function.child_by_field_name("name") {
        return Some(own_name.utf8_text(bytes).unwrap_or_default().to_string());
    }

    let declarator = function.parent()?;

    if declarator.kind() != "variable_declarator" {
        return None;
    }

    let name_node = declarator.child_by_field_name("name")?;

    (name_node.kind() == "identifier")
        .then(|| name_node.utf8_text(bytes).unwrap_or_default().to_string())
}

/// Capture indices extracted from the query, grouped for readability
struct FrontendCaptures {
    func_name: Option<u32>,
    arg_value: Option<u32>,
    func_name_second: Option<u32>,
    arg_value_second: Option<u32>,
    imported_name: Option<u32>,
    local_alias: Option<u32>,
    import_source: Option<u32>,
    call_generic: Option<u32>,
    call_await_generic: Option<u32>,
    specta_method_name: Option<u32>,
    specta_call: Option<u32>,
    specta_event_name: Option<u32>,
    specta_event_method: Option<u32>,
    plain_fn: Option<u32>,
    plain_args: Option<u32>,
}

impl FrontendCaptures {
    fn from_query(query: &Query) -> Self {
        Self {
            func_name: query.capture_index_for_name("func_name"),
            arg_value: query.capture_index_for_name("arg_value"),
            func_name_second: query.capture_index_for_name("func_name_second"),
            arg_value_second: query.capture_index_for_name("arg_value_second"),
            imported_name: query.capture_index_for_name("imported_name"),
            local_alias: query.capture_index_for_name("local_alias"),
            import_source: query.capture_index_for_name("import_source"),
            call_generic: query.capture_index_for_name("call_generic"),
            call_await_generic: query.capture_index_for_name("call_await_generic"),
            specta_method_name: query.capture_index_for_name("specta_method_name"),
            specta_call: query.capture_index_for_name("specta_call"),
            specta_event_name: query.capture_index_for_name("specta_event_name"),
            specta_event_method: query.capture_index_for_name("specta_event_method"),
            plain_fn: query.capture_index_for_name("plain_fn"),
            plain_args: query.capture_index_for_name("plain_args"),
        }
    }
}

/// Parse TypeScript/JavaScript source code
///
/// # Errors
///
/// Returns error if tree-sitter query execution fails.
#[allow(clippy::implicit_hasher)]
pub fn parse_frontend(
    content: &str,
    lang: LangType,
    line_offset: usize,
    global_constants: &HashMap<String, String>,
    known_forwarders: &HashMap<String, Forwarder>,
) -> ParseResult<FrontendParse> {
    let ts_lang: Language = match lang {
        LangType::JavaScript | LangType::JavaScriptJsx => tree_sitter_javascript::LANGUAGE.into(),
        LangType::TypeScriptJsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };

    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set {lang:?} language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError(format!("Failed to parse {lang:?} file")))?;

    // tree-sitter is error-tolerant: a file that is broken mid-edit still parses
    // into a tree with ERROR/MISSING nodes rather than returning None. Indexing
    // such a tree yields partial findings — e.g. a half-typed `listen<...>()` lands
    // in an ERROR node and is dropped while a valid `emit()` for the same event is
    // still captured, producing a spurious "emitted but no listeners" diagnostic.
    // Treat any syntax error as a parse failure so the pipeline keeps the file's
    // last valid index (and suppresses diagnostics) until it parses cleanly again.
    if tree.root_node().has_error() {
        return Err(ParseError::SyntaxError(format!(
            "{lang:?} file has syntax errors; keeping last valid index"
        )));
    }

    let query_src = get_query_source(lang);
    let query = Query::new(&ts_lang, query_src)
        .map_err(|e| ParseError::QueryError(format!("Failed to create {lang:?} query: {e}")))?;

    let caps = FrontendCaptures::from_query(&query);
    let root = tree.root_node();
    let bytes = content.as_bytes();

    // First pass: collect import aliases
    let aliases = collect_aliases(&query, root, bytes, &caps);

    // Local constants win over workspace-wide ones, and locally bound identifiers
    // (parameters, destructured variables) win over both — see `Scope`.
    let is_js = matches!(lang, LangType::JavaScript | LangType::JavaScriptJsx);
    let scope = Scope {
        local_constants: crate::utils::extract_js_constants(root, content, is_js),
        global_constants,
        bound_names: crate::utils::collect_bound_names(root, content),
    };

    // Second pass: collect function calls
    let mut findings = Vec::new();
    let mut dynamic_usages = DynamicUsages::default();
    let mut forwarders = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, bytes);

    while let Some(m) = matches.next() {
        match process_first_arg_pattern(m, &caps, bytes, &aliases, &scope, content, line_offset) {
            PatternOutcome::Found(f) => findings.push(f),
            PatternOutcome::Forwards(fwd) => forwarders.push(fwd),
            PatternOutcome::Dynamic(behavior) => dynamic_usages.record(behavior),
            PatternOutcome::NoMatch => {}
        }
        match process_second_arg_pattern(m, &caps, bytes, &aliases, &scope, line_offset) {
            PatternOutcome::Found(f) => findings.push(f),
            PatternOutcome::Forwards(fwd) => forwarders.push(fwd),
            PatternOutcome::Dynamic(behavior) => dynamic_usages.record(behavior),
            PatternOutcome::NoMatch => {}
        }
        if let Some(f) = process_specta_call(m, &caps, bytes, content, line_offset) {
            findings.push(f);
        }
        if let Some(f) = process_specta_event(m, &caps, bytes, line_offset) {
            findings.push(f);
        }
        match process_forwarder_call(m, &caps, bytes, &scope, known_forwarders, line_offset) {
            PatternOutcome::Found(f) => findings.push(f),
            PatternOutcome::Dynamic(behavior) => dynamic_usages.record(behavior),
            PatternOutcome::Forwards(_) | PatternOutcome::NoMatch => {}
        }
    }

    Ok(FrontendParse {
        findings,
        dynamic_usages,
        forwarders,
    })
}

/// Collect only the forwarding helpers a file declares.
///
/// The main pass cannot do this on its own: a call site is resolvable only once the
/// helper it calls is known, and helpers are routinely declared in another file.
/// This runs alongside the constant pre-pass and deliberately skips everything the
/// declaration scan does not need — constants, bound names, specta, findings.
///
/// # Errors
///
/// Returns error if tree-sitter fails to parse the file or query execution fails.
pub fn extract_forwarders(content: &str, lang: LangType) -> ParseResult<Vec<Forwarder>> {
    let ts_lang: Language = match lang {
        LangType::JavaScript | LangType::JavaScriptJsx => tree_sitter_javascript::LANGUAGE.into(),
        LangType::TypeScriptJsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };

    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set {lang:?} language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError(format!("Failed to parse {lang:?} file")))?;

    if tree.root_node().has_error() {
        return Ok(Vec::new());
    }

    let query = Query::new(&ts_lang, get_query_source(lang))
        .map_err(|e| ParseError::QueryError(format!("Failed to create {lang:?} query: {e}")))?;

    let caps = FrontendCaptures::from_query(&query);
    let root = tree.root_node();
    let bytes = content.as_bytes();
    let aliases = collect_aliases(&query, root, bytes, &caps);

    let mut forwarders = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, bytes);

    while let Some(m) = matches.next() {
        for (func_cap, arg_cap, position) in [
            (caps.func_name, caps.arg_value, ArgPosition::First),
            (
                caps.func_name_second,
                caps.arg_value_second,
                ArgPosition::Second,
            ),
        ] {
            let (Some(func_cap), Some(arg_cap)) =
                (find_capture(m, func_cap), find_capture(m, arg_cap))
            else {
                continue;
            };

            // Only a bare identifier can be a forwarded parameter.
            if arg_cap.node.kind() != "identifier" {
                continue;
            }

            let func_name = func_cap.node.utf8_text(bytes).unwrap_or_default();
            let Some(original_name) = aliases.get(func_name).copied() else {
                continue;
            };

            let Some(pattern) = ALL_FRONTEND_PATTERNS
                .iter()
                .find(|p| p.name == original_name && p.arg_position == position)
            else {
                continue;
            };

            let raw = arg_cap.node.utf8_text(bytes).unwrap_or_default();

            if let Some(forwarder) =
                detect_forwarder(arg_cap.node, raw, bytes, pattern.entity, pattern.behavior)
            {
                forwarders.push(forwarder);
            }
        }
    }

    Ok(forwarders)
}

/// Turn a call of a known forwarding helper into a finding for the real command
/// or event, anchored at the literal in *this* file.
///
/// This is what makes `emitToOverlays("units-changed", value)` navigable: the
/// finding is indexed exactly as if `emit("units-changed", ...)` were written here.
fn process_forwarder_call(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    scope: &Scope<'_>,
    known_forwarders: &HashMap<String, Forwarder>,
    line_offset: usize,
) -> PatternOutcome {
    let (Some(fn_cap), Some(args_cap)) = (
        find_capture(m, caps.plain_fn),
        find_capture(m, caps.plain_args),
    ) else {
        return PatternOutcome::NoMatch;
    };

    let called = fn_cap.node.utf8_text(bytes).unwrap_or_default();
    let Some(forwarder) = known_forwarders.get(called) else {
        return PatternOutcome::NoMatch;
    };

    // A helper declared in this file is also matched here; that is intended, since
    // its own body forwards a parameter and produces no finding of its own.
    let mut cursor = args_cap.node.walk();
    let Some(name_arg) = args_cap
        .node
        .named_children(&mut cursor)
        .nth(forwarder.param_index)
    else {
        return PatternOutcome::Dynamic(forwarder.behavior);
    };

    match resolve_name_argument(name_arg, bytes, scope) {
        ArgResolution::Resolved(name, range) => PatternOutcome::Found(Finding::new(
            name,
            forwarder.entity,
            forwarder.behavior,
            adjust_range(range, line_offset),
        )),
        // A forwarder fed by another forwarder's parameter would need a second
        // hop; one level is resolved, deeper chains stay unprovable.
        ArgResolution::Unresolved => PatternOutcome::Dynamic(forwarder.behavior),
        ArgResolution::EmptyLiteral => PatternOutcome::NoMatch,
    }
}

fn collect_aliases<'a>(
    query: &Query,
    root: tree_sitter::Node<'_>,
    bytes: &'a [u8],
    caps: &FrontendCaptures,
) -> HashMap<&'a str, &'a str> {
    let mut aliases = HashMap::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, bytes);

    while let Some(m) = matches.next() {
        let src_cap = find_capture(m, caps.import_source);

        if let Some(src_node) = src_cap {
            let source = src_node.node.utf8_text(bytes).unwrap_or_default();

            if source.starts_with("@tauri-apps/") {
                let imp = find_capture(m, caps.imported_name);
                let loc = find_capture(m, caps.local_alias);

                if let (Some(imp_cap), Some(loc_cap)) = (imp, loc) {
                    let imported = imp_cap.node.utf8_text(bytes).unwrap_or_default();
                    let local = loc_cap.node.utf8_text(bytes).unwrap_or_default();

                    aliases.insert(local, imported);
                } else if let Some(imp_cap) = imp {
                    let imported = imp_cap.node.utf8_text(bytes).unwrap_or_default();

                    aliases.insert(imported, imported);
                }
            }
        }
    }

    aliases
}

/// Read the string a name argument denotes, plus the range to attach findings to.
///
/// Outcome of reading the name argument of a Tauri call.
enum ArgResolution {
    /// A name was recovered, together with the range to anchor findings to.
    Resolved(String, Range),
    /// The argument is a literal but names nothing — `emit("")`. There is no
    /// entity to index and, crucially, nothing unknown either: an empty literal
    /// must not weaken the diagnostics the way a genuinely dynamic name does.
    EmptyLiteral,
    /// The argument is not a literal and cannot be traced to one.
    Unresolved,
}

fn resolve_name_argument(
    arg: tree_sitter::Node<'_>,
    bytes: &[u8],
    scope: &Scope<'_>,
) -> ArgResolution {
    let raw = arg.utf8_text(bytes).unwrap_or_default();

    let range = Range {
        start: point_to_position(arg.start_position()),
        end: point_to_position(arg.end_position()),
    };

    if arg.kind() != "string" {
        return scope
            .resolve(raw)
            .map_or(ArgResolution::Unresolved, |name| {
                ArgResolution::Resolved(name, range)
            });
    }

    if let Some(fragment) = arg.named_child(0) {
        return ArgResolution::Resolved(
            fragment.utf8_text(bytes).unwrap_or_default().to_string(),
            Range {
                start: point_to_position(fragment.start_position()),
                end: point_to_position(fragment.end_position()),
            },
        );
    }

    // An empty literal has no fragment child, so strip the quotes to confirm it.
    let unquoted = raw
        .strip_prefix(['"', '\'', '`'])
        .and_then(|s| s.strip_suffix(['"', '\'', '`']))
        .unwrap_or(raw);

    if unquoted.is_empty() {
        return ArgResolution::EmptyLiteral;
    }

    ArgResolution::Resolved(unquoted.to_string(), range)
}

fn process_first_arg_pattern<'a>(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &'a [u8],
    aliases: &HashMap<&'a str, &'a str>,
    scope: &Scope<'_>,
    content: &str,
    line_offset: usize,
) -> PatternOutcome {
    let (Some(func_cap), Some(arg_cap)) = (
        find_capture(m, caps.func_name),
        find_capture(m, caps.arg_value),
    ) else {
        return PatternOutcome::NoMatch;
    };

    let func_name = func_cap.node.utf8_text(bytes).unwrap_or_default();
    let Some(original_name) = aliases.get(func_name).copied() else {
        return PatternOutcome::NoMatch;
    };

    let Some(pattern) = ALL_FRONTEND_PATTERNS
        .iter()
        .find(|p| p.name == original_name && p.arg_position == ArgPosition::First)
    else {
        return PatternOutcome::NoMatch;
    };

    // `invoke` is deliberately literal-only — a command name is never resolved
    // through the constant table. A forwarded parameter is different: the literal
    // does exist, at the helper's call sites, so it is still worth recovering.
    if original_name == "invoke" && arg_cap.node.kind() != "string" {
        let raw = arg_cap.node.utf8_text(bytes).unwrap_or_default();

        return detect_forwarder(arg_cap.node, raw, bytes, pattern.entity, pattern.behavior)
            .map_or(PatternOutcome::Dynamic(pattern.behavior), |forwarder| {
                PatternOutcome::Forwards(forwarder)
            });
    }

    let (resolved_arg, range) = match resolve_name_argument(arg_cap.node, bytes, scope) {
        ArgResolution::Resolved(name, range) => (name, range),
        ArgResolution::Unresolved => {
            let raw = arg_cap.node.utf8_text(bytes).unwrap_or_default();

            return detect_forwarder(arg_cap.node, raw, bytes, pattern.entity, pattern.behavior)
                .map_or(PatternOutcome::Dynamic(pattern.behavior), |forwarder| {
                    PatternOutcome::Forwards(forwarder)
                });
        }
        ArgResolution::EmptyLiteral => return PatternOutcome::NoMatch,
    };

    let call_name_end = Some(adjust_position(
        point_to_position(func_cap.node.end_position()),
        line_offset,
    ));
    let type_arg_info =
        extract_type_argument_info(m, caps.call_generic, caps.call_await_generic, content);
    let return_type = type_arg_info.as_ref().map(|i| i.type_text.clone());
    let type_arg_range = type_arg_info.map(|i| adjust_range(i.type_arg_range, line_offset));

    PatternOutcome::Found(Finding {
        return_type,
        call_name_end,
        type_arg_range,
        ..Finding::new(
            resolved_arg,
            pattern.entity,
            pattern.behavior,
            adjust_range(range, line_offset),
        )
    })
}

fn process_second_arg_pattern<'a>(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &'a [u8],
    aliases: &HashMap<&'a str, &'a str>,
    scope: &Scope<'_>,
    line_offset: usize,
) -> PatternOutcome {
    let (Some(func_cap), Some(arg_cap)) = (
        find_capture(m, caps.func_name_second),
        find_capture(m, caps.arg_value_second),
    ) else {
        return PatternOutcome::NoMatch;
    };

    let func_name = func_cap.node.utf8_text(bytes).unwrap_or_default();
    let Some(original_name) = aliases.get(func_name).copied() else {
        return PatternOutcome::NoMatch;
    };

    let Some(pattern) = ALL_FRONTEND_PATTERNS
        .iter()
        .find(|p| p.name == original_name && p.arg_position == ArgPosition::Second)
    else {
        return PatternOutcome::NoMatch;
    };

    let (resolved_arg, range) = match resolve_name_argument(arg_cap.node, bytes, scope) {
        ArgResolution::Resolved(name, range) => (name, range),
        ArgResolution::Unresolved => {
            let raw = arg_cap.node.utf8_text(bytes).unwrap_or_default();

            return detect_forwarder(arg_cap.node, raw, bytes, pattern.entity, pattern.behavior)
                .map_or(PatternOutcome::Dynamic(pattern.behavior), |forwarder| {
                    PatternOutcome::Forwards(forwarder)
                });
        }
        ArgResolution::EmptyLiteral => return PatternOutcome::NoMatch,
    };

    PatternOutcome::Found(Finding::new(
        resolved_arg,
        pattern.entity,
        pattern.behavior,
        adjust_range(range, line_offset),
    ))
}

fn process_specta_call(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    content: &str,
    line_offset: usize,
) -> Option<Finding> {
    let specta_cap = find_capture(m, caps.specta_method_name)?;

    let camel_name = specta_cap.node.utf8_text(bytes).unwrap_or_default();
    let snake_name = crate::utils::camel_to_snake(camel_name);
    let method_range = Range {
        start: point_to_position(specta_cap.node.start_position()),
        end: point_to_position(specta_cap.node.end_position()),
    };
    let arg_count = count_specta_call_args(m, caps.specta_call, content);

    Some(Finding {
        call_arg_count: Some(arg_count),
        ..Finding::new(
            snake_name,
            EntityType::Command,
            Behavior::SpectaCall,
            adjust_range(method_range, line_offset),
        )
    })
}

fn process_specta_event(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    line_offset: usize,
) -> Option<Finding> {
    let name_cap = find_capture(m, caps.specta_event_name)?;
    let method_cap = find_capture(m, caps.specta_event_method)?;

    let camel_name = name_cap.node.utf8_text(bytes).unwrap_or_default();
    let method_name = method_cap.node.utf8_text(bytes).unwrap_or_default();

    let behavior = match method_name {
        "emit" => Behavior::Emit,
        "listen" | "once" => Behavior::Listen,
        _ => return None,
    };

    let kebab_name = crate::utils::camel_to_kebab(camel_name);
    let name_range = Range {
        start: point_to_position(name_cap.node.start_position()),
        end: point_to_position(name_cap.node.end_position()),
    };

    Some(Finding {
        codegen_origin: Some(crate::indexer::GeneratorKind::Specta),
        ..Finding::new(
            kebab_name,
            EntityType::Event,
            behavior,
            adjust_range(name_range, line_offset),
        )
    })
}
