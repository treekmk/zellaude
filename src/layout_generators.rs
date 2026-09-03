//! Generator files: one KDL file per generator under the plugin's `generators`
//! directory, re-parsed on every prompt open. A file declares its prompt-line
//! vocabulary, then a body of `tab`/`pane`/`each` nodes that expands into the
//! tabs an invocation opens.

use kdl::{KdlDocument, KdlNode, KdlValue};
use serde::Deserialize;
use serde_json::{Map, Value};

pub const DEFAULT_MIN_PANE_WIDTH: usize = 54; // content columns
pub const DEFAULT_MIN_PANE_HEIGHT: usize = 12; // content rows
pub const PANE_FRAME_COLUMNS: usize = 2; // Zellij frame cost per pane
pub const PANE_FRAME_ROWS: usize = 2;

/// Bound to the name of the tab the prompt was opened from.
const SOURCE_TAB_VARIABLE: &str = "tab";

const BODY_NODES: [&str; 3] = ["tab", "pane", "each"];

const DECLARATION_NODES: [&str; 5] = [
    "command",
    "arg",
    "flag",
    "min_pane_width",
    "min_pane_height",
];

#[derive(Debug, Deserialize)]
pub struct GeneratorFile {
    pub path: String,
    pub content: String,
}

/// The envelope the reload script prints: the raw zellaude.json text and every
/// generator file, in `LC_ALL=C` name order.
#[derive(Debug, Deserialize)]
pub struct CustomStateSources {
    pub settings_json: String,
    pub generator_files: Vec<GeneratorFile>,
}

/// One file's or zellaude.json's partial floors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FloorOverrides {
    pub min_pane_width: Option<usize>,
    pub min_pane_height: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFloors {
    pub min_pane_width: usize,
    pub min_pane_height: usize,
}

impl PaneFloors {
    pub fn resolve(file: FloorOverrides, global: FloorOverrides) -> Self {
        Self {
            min_pane_width: file
                .min_pane_width
                .or(global.min_pane_width)
                .unwrap_or(DEFAULT_MIN_PANE_WIDTH),
            min_pane_height: file
                .min_pane_height
                .or(global.min_pane_height)
                .unwrap_or(DEFAULT_MIN_PANE_HEIGHT),
        }
    }
}

/// Read the floor keys from the top level of zellaude.json. A document that is
/// not an object carries no floors; the custom-state parser reports its shape.
pub fn parse_floor_overrides(settings_json: &str) -> Result<FloorOverrides, String> {
    let Value::Object(object) =
        serde_json::from_str(settings_json).map_err(|error| error.to_string())?
    else {
        return Ok(FloorOverrides::default());
    };
    Ok(FloorOverrides {
        min_pane_width: floor_key(&object, "min_pane_width")?,
        min_pane_height: floor_key(&object, "min_pane_height")?,
    })
}

fn floor_key(object: &Map<String, Value>, key: &str) -> Result<Option<usize>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|floor| usize::try_from(floor).ok())
            .map(Some)
            .ok_or_else(|| format!("zellaude.json {key} must be a non-negative integer")),
    }
}

/// Cells the grid may use: the display area minus the bar rows the new tab
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabGeometry {
    pub columns: usize,
    pub rows: usize,
}

/// The tab the prompt was opened from: bound as `{tab}`, and the size every
/// generated tab is planned against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTab {
    pub name: String,
    pub geometry: TabGeometry,
}

#[derive(Debug)]
pub struct LayoutGenerator {
    pub command: String,
    /// File basename, so every refusal names the file that caused it.
    pub source: String,
    args: Vec<String>,
    flags: Vec<FlagDeclaration>,
    floors: FloorOverrides,
    body: Vec<BodyNode>,
}

#[derive(Debug)]
struct FlagDeclaration {
    /// As typed on the prompt line, without the leading `--`.
    flag: String,
    presence: String,
    value: Option<FlagValue>,
}

#[derive(Debug)]
struct FlagValue {
    variable: String,
    /// `--flag` may stand alone; `default` then supplies the integer.
    optional: bool,
    /// Absent means the flag itself is required on the prompt line.
    default: Option<i64>,
}

#[derive(Debug)]
enum BodyNode {
    Tab {
        name: Option<Vec<TemplatePiece>>,
        condition: Condition,
        body: Vec<BodyNode>,
    },
    Pane {
        command: Vec<TemplatePiece>,
        condition: Condition,
    },
    Each {
        variable: String,
        range: Range,
        condition: Condition,
        body: Vec<BodyNode>,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum TemplatePiece {
    Literal(String),
    Variable(String),
}

#[derive(Debug, Default)]
struct Condition {
    required: Vec<String>,
    forbidden: Vec<String>,
}

#[derive(Debug)]
struct Range {
    start: Endpoint,
    end: Endpoint,
    inclusive: bool,
}

#[derive(Debug)]
struct Endpoint {
    head: Term,
    tail: Option<(Sign, Term)>,
}

#[derive(Debug)]
enum Term {
    Literal(i64),
    Variable(String),
}

#[derive(Debug, Clone, Copy)]
enum Sign {
    Plus,
    Minus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VariableKind {
    Integer,
    Presence,
    /// The built-in `tab` -- the only string, and never an operand.
    Text,
}

/// The variable names visible at one point of a file, innermost last.
#[derive(Debug)]
struct Scope(Vec<(String, VariableKind)>);

impl Scope {
    fn with_source_tab() -> Self {
        Self(vec![(
            SOURCE_TAB_VARIABLE.to_string(),
            VariableKind::Text,
        )])
    }

    fn declare(&mut self, name: &str, kind: VariableKind) -> Result<(), String> {
        validate_variable_name(name)?;
        if self.kind(name).is_some() {
            return Err(format!("variable {name:?} is already declared"));
        }
        self.0.push((name.to_string(), kind));
        Ok(())
    }

    fn kind(&self, name: &str) -> Option<VariableKind> {
        self.0
            .iter()
            .find(|(declared, _)| declared == name)
            .map(|(_, kind)| *kind)
    }

    fn depth(&self) -> usize {
        self.0.len()
    }

    fn unwind(&mut self, depth: usize) {
        self.0.truncate(depth);
    }
}

pub fn parse_generator_files(files: &[GeneratorFile]) -> Result<Vec<LayoutGenerator>, String> {
    let mut generators: Vec<LayoutGenerator> = Vec::with_capacity(files.len());
    for file in files {
        let source = basename(&file.path);
        let generator = parse_generator(&file.content, source)
            .map_err(|message| format!("{source}: {message}"))?;
        if let Some(earlier) = generators
            .iter()
            .find(|other| other.command == generator.command)
        {
            return Err(format!(
                "{source}: command {:?} is already declared by {}",
                generator.command, earlier.source
            ));
        }
        generators.push(generator);
    }
    Ok(generators)
}

/// The variable values one prompt line binds: positionals, flag values and the
/// flags that were given. Every name binds at most once -- a repeated flag
/// refuses, and the parser already refused a duplicate declaration.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Bindings {
    integers: Vec<(String, i64)>,
    presences: Vec<String>,
}

impl Bindings {
    pub(crate) fn integer(&self, name: &str) -> Option<i64> {
        self.integers
            .iter()
            .find(|(bound, _)| bound == name)
            .map(|(_, value)| *value)
    }

    pub(crate) fn is_present(&self, name: &str) -> bool {
        self.presences.iter().any(|bound| bound == name)
    }
}

/// Match the line's first whitespace-separated token against the declared
/// commands, returning the generator and the tokens left to bind.
pub(crate) fn select_generator<'a, 'b>(
    generators: &'a [LayoutGenerator],
    input: &'b str,
) -> Option<(&'a LayoutGenerator, Vec<&'b str>)> {
    let mut tokens = input.split_whitespace();
    let command = tokens.next()?;
    let generator = generators
        .iter()
        .find(|generator| generator.command == command)?;
    Some((generator, tokens.collect()))
}

impl LayoutGenerator {
    /// Bind the tokens after the command: positionals in declaration order,
    /// flags in any order, defaults for whatever the line left out.
    pub(crate) fn bind_arguments(&self, tokens: &[&str]) -> Result<Bindings, String> {
        let mut bindings = Bindings::default();
        let mut positionals: Vec<(&str, i64)> = Vec::new();
        let mut index = 0;
        while let Some(token) = tokens.get(index) {
            index += 1;
            let Some(name) = token.strip_prefix("--") else {
                positionals.push((token, parse_integer_token(token)?));
                continue;
            };
            let flag = self
                .flags
                .iter()
                .find(|declared| declared.flag == name)
                .ok_or_else(|| format!("unknown flag --{name}"))?;
            if bindings.is_present(&flag.presence) {
                return Err(format!("flag --{name} is given more than once"));
            }
            bindings.presences.push(flag.presence.clone());
            let Some(value) = &flag.value else { continue };
            match tokens.get(index).and_then(|token| token.parse::<i64>().ok()) {
                Some(number) => {
                    bindings.integers.push((value.variable.clone(), number));
                    index += 1;
                }
                None if value.optional => {}
                None => {
                    return Err(match tokens.get(index) {
                        Some(token) => {
                            format!("flag --{name} needs an integer value, got {token:?}")
                        }
                        None => format!("flag --{name} needs an integer value"),
                    })
                }
            }
        }

        for (position, name) in self.args.iter().enumerate() {
            let Some((_, value)) = positionals.get(position) else {
                return Err(format!("missing argument {name:?}"));
            };
            bindings.integers.push((name.clone(), *value));
        }
        if let Some((token, _)) = positionals.get(self.args.len()) {
            return Err(format!("unexpected argument {token:?}"));
        }

        for flag in &self.flags {
            let Some(value) = &flag.value else { continue };
            if bindings.integer(&value.variable).is_some() {
                continue;
            }
            match value.default {
                Some(default) => bindings.integers.push((value.variable.clone(), default)),
                None => return Err(format!("missing required flag --{}", flag.flag)),
            }
        }
        Ok(bindings)
    }
}

fn parse_integer_token(token: &str) -> Result<i64, String> {
    token
        .parse::<i64>()
        .map_err(|_| format!("argument {token:?} must be an integer"))
}

fn parse_generator(content: &str, source: &str) -> Result<LayoutGenerator, String> {
    let document = content
        .parse::<KdlDocument>()
        .map_err(|error| format!("invalid KDL: {error}"))?;

    let mut declarations = Declarations::new();
    let mut body_nodes: Vec<&KdlNode> = Vec::new();
    for node in document.nodes() {
        let name = node.name().value();
        if BODY_NODES.contains(&name) {
            body_nodes.push(node);
        } else if DECLARATION_NODES.contains(&name) {
            if !body_nodes.is_empty() {
                return Err(format!(
                    "{name:?} must be declared before the first tab, pane or each node"
                ));
            }
            declarations.declare(node)?;
        } else {
            return Err(format!("unknown node {name:?}"));
        }
    }

    let command = declarations
        .command
        .ok_or_else(|| "a generator file must declare a command".to_string())?;
    let body = parse_body(&body_nodes, &mut declarations.scope, false)?;
    Ok(LayoutGenerator {
        command,
        source: source.to_string(),
        args: declarations.args,
        flags: declarations.flags,
        floors: declarations.floors,
        body,
    })
}

/// What the declaration nodes of one file build up.
#[derive(Debug)]
struct Declarations {
    command: Option<String>,
    args: Vec<String>,
    flags: Vec<FlagDeclaration>,
    floors: FloorOverrides,
    scope: Scope,
}

impl Declarations {
    fn new() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            flags: Vec::new(),
            floors: FloorOverrides::default(),
            scope: Scope::with_source_tab(),
        }
    }

    fn declare(&mut self, node: &KdlNode) -> Result<(), String> {
        reject_children(node)?;
        reject_unknown_properties(node)?;
        match node.name().value() {
            "command" => {
                if self.command.is_some() {
                    return Err("command is declared more than once".to_string());
                }
                let command = single_string_argument(node)?;
                if command.split_whitespace().count() != 1 {
                    return Err(format!("command {command:?} must be a single word"));
                }
                self.command = Some(command.to_string());
            }
            "arg" => {
                let name = single_string_argument(node)?;
                self.scope.declare(name, VariableKind::Integer)?;
                self.args.push(name.to_string());
            }
            "flag" => self.declare_flag(node)?,
            "min_pane_width" => set_floor(&mut self.floors.min_pane_width, node)?,
            _ => set_floor(&mut self.floors.min_pane_height, node)?,
        }
        Ok(())
    }

    fn declare_flag(&mut self, node: &KdlNode) -> Result<(), String> {
        let flag = single_string_argument(node)?;
        validate_flag_name(flag)?;
        let value = parse_flag_value(node, flag)?;
        let presence = flag.replace('-', "_");
        self.scope.declare(&presence, VariableKind::Presence)?;
        if let Some(value) = &value {
            self.scope.declare(&value.variable, VariableKind::Integer)?;
        }
        self.flags.push(FlagDeclaration {
            flag: flag.to_string(),
            presence,
            value,
        });
        Ok(())
    }
}

fn parse_flag_value(node: &KdlNode, flag: &str) -> Result<Option<FlagValue>, String> {
    let required_value = property_string(node, "value")?;
    let optional_value = property_string(node, "optional-value")?;
    let default = property_integer(node, "default")?;
    match (required_value, optional_value) {
        (Some(_), Some(_)) => Err(format!(
            "flag {flag:?} sets both value and optional-value"
        )),
        (Some(variable), None) => Ok(Some(FlagValue {
            variable: variable.to_string(),
            optional: false,
            default,
        })),
        (None, Some(variable)) => {
            if default.is_none() {
                return Err(format!(
                    "flag {flag:?} sets optional-value and needs a default, since {:?} would be unbound when the flag stands alone",
                    variable
                ));
            }
            Ok(Some(FlagValue {
                variable: variable.to_string(),
                optional: true,
                default,
            }))
        }
        (None, None) if default.is_some() => {
            Err(format!("flag {flag:?} sets a default without a value"))
        }
        (None, None) => Ok(None),
    }
}

fn set_floor(slot: &mut Option<usize>, node: &KdlNode) -> Result<(), String> {
    let name = node.name().value();
    if slot.is_some() {
        return Err(format!("{name} is declared more than once"));
    }
    *slot = Some(single_usize_argument(node)?);
    Ok(())
}

fn parse_body(
    nodes: &[&KdlNode],
    scope: &mut Scope,
    inside_tab: bool,
) -> Result<Vec<BodyNode>, String> {
    let mut body = Vec::with_capacity(nodes.len());
    for node in nodes {
        body.push(parse_body_node(node, scope, inside_tab)?);
    }
    Ok(body)
}

fn parse_body_node(
    node: &KdlNode,
    scope: &mut Scope,
    inside_tab: bool,
) -> Result<BodyNode, String> {
    let name = node.name().value();
    if !BODY_NODES.contains(&name) {
        return Err(format!("unknown node {name:?}"));
    }
    reject_unknown_properties(node)?;
    let condition = parse_condition(node, scope)?;
    match name {
        "tab" => {
            if inside_tab {
                return Err("a tab node must not be nested inside another tab".to_string());
            }
            let name = match node_arguments(node).as_slice() {
                [] => None,
                [value] => Some(parse_template(expect_string(value, "a tab name")?, scope)?),
                _ => return Err("a tab node takes at most one name argument".to_string()),
            };
            Ok(BodyNode::Tab {
                name,
                condition,
                body: parse_body(&child_nodes(node), scope, true)?,
            })
        }
        "pane" => {
            if !inside_tab {
                return Err("a pane node must be inside a tab".to_string());
            }
            reject_children(node)?;
            let [value] = node_arguments(node)[..] else {
                return Err("a pane node takes exactly one command argument".to_string());
            };
            Ok(BodyNode::Pane {
                command: parse_template(expect_string(value, "a pane command")?, scope)?,
                condition,
            })
        }
        _ => {
            if !node_arguments(node).is_empty() {
                return Err("an each node takes no arguments".to_string());
            }
            let variable = required_property_string(node, "for")?;
            let range = parse_range(required_property_string(node, "in")?, scope)?;
            let depth = scope.depth();
            scope.declare(variable, VariableKind::Integer)?;
            let body = parse_body(&child_nodes(node), scope, inside_tab)?;
            scope.unwind(depth);
            Ok(BodyNode::Each {
                variable: variable.to_string(),
                range,
                condition,
                body,
            })
        }
    }
}

fn parse_condition(node: &KdlNode, scope: &Scope) -> Result<Condition, String> {
    Ok(Condition {
        required: parse_presence_list(node, "if", scope)?,
        forbidden: parse_presence_list(node, "unless", scope)?,
    })
}

fn parse_presence_list(
    node: &KdlNode,
    property: &str,
    scope: &Scope,
) -> Result<Vec<String>, String> {
    let Some(raw) = property_string(node, property)? else {
        return Ok(Vec::new());
    };
    raw.split_whitespace()
        .map(|name| match scope.kind(name) {
            Some(VariableKind::Presence) => Ok(name.to_string()),
            Some(_) => Err(format!(
                "variable {name:?} in {property} is not a flag presence"
            )),
            None => Err(format!("unknown variable {name:?} in {property}")),
        })
        .collect()
}

fn parse_range(raw: &str, scope: &Scope) -> Result<Range, String> {
    let raw = raw.trim();
    let Some(separator) = raw.find("..") else {
        return Err(format!("range {raw:?} needs a '..' or '..=' separator"));
    };
    let rest = &raw[separator + 2..];
    let (inclusive, end) = match rest.strip_prefix('=') {
        Some(end) => (true, end),
        None => (false, rest),
    };
    Ok(Range {
        start: parse_endpoint(&raw[..separator], scope)?,
        end: parse_endpoint(end, scope)?,
        inclusive,
    })
}

fn parse_endpoint(raw: &str, scope: &Scope) -> Result<Endpoint, String> {
    let raw = raw.trim();
    let mut operators = raw
        .char_indices()
        .filter(|(_, character)| matches!(character, '+' | '-'));
    let operator = operators.next();
    if operators.next().is_some() {
        return Err(format!("range bound {raw:?} may use at most one '+' or '-'"));
    }
    let (head, tail) = match operator {
        Some((0, _)) => {
            return Err(format!(
                "range bound {raw:?} must start with a number or a variable"
            ))
        }
        Some((index, character)) => {
            let sign = if character == '+' { Sign::Plus } else { Sign::Minus };
            let term = parse_term(&raw[index + 1..], scope)?;
            (&raw[..index], Some((sign, term)))
        }
        None => (raw, None),
    };
    Ok(Endpoint {
        head: parse_term(head, scope)?,
        tail,
    })
}

fn parse_term(raw: &str, scope: &Scope) -> Result<Term, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("a range bound is empty".to_string());
    }
    if raw.chars().all(|character| character.is_ascii_digit()) {
        return raw
            .parse::<i64>()
            .map(Term::Literal)
            .map_err(|_| format!("range bound {raw:?} is too large"));
    }
    match scope.kind(raw) {
        Some(VariableKind::Integer) => Ok(Term::Variable(raw.to_string())),
        Some(_) => Err(format!("range bound {raw:?} is not an integer variable")),
        None => Err(format!("unknown variable {raw:?} in a range")),
    }
}

/// Split a name or command into literals and substitutions. Only a brace group
/// naming a declared integer or `tab` substitutes, so shell syntax such as
/// `${HOME}` and `{a,b}` survives verbatim.
fn parse_template(raw: &str, scope: &Scope) -> Result<Vec<TemplatePiece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut rest = raw;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let name = after_open
            .find('}')
            .map(|close| (&after_open[..close], close));
        match name.map(|(name, close)| (name, close, scope.kind(name))) {
            Some((name, _, Some(VariableKind::Presence))) => {
                return Err(format!(
                    "variable {name:?} is a flag presence and is legal only in if or unless"
                ))
            }
            Some((name, close, Some(_))) => {
                literal.push_str(&rest[..open]);
                if !literal.is_empty() {
                    pieces.push(TemplatePiece::Literal(std::mem::take(&mut literal)));
                }
                pieces.push(TemplatePiece::Variable(name.to_string()));
                rest = &after_open[close + 1..];
            }
            _ => {
                literal.push_str(&rest[..=open]);
                rest = after_open;
            }
        }
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        pieces.push(TemplatePiece::Literal(literal));
    }
    Ok(pieces)
}

fn validate_variable_name(name: &str) -> Result<(), String> {
    let mut characters = name.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid {
        return Err(format!(
            "variable name {name:?} must be a letter or '_' followed by letters, digits or '_'"
        ));
    }
    Ok(())
}

fn validate_flag_name(flag: &str) -> Result<(), String> {
    let mut characters = flag.chars();
    let valid = characters.next().is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid {
        return Err(format!(
            "flag name {flag:?} must be a letter followed by letters, digits, '-' or '_'"
        ));
    }
    Ok(())
}

fn allowed_properties(node_name: &str) -> &'static [&'static str] {
    match node_name {
        "flag" => &["value", "optional-value", "default"],
        "tab" | "pane" => &["if", "unless"],
        "each" => &["for", "in", "if", "unless"],
        _ => &[],
    }
}

fn reject_unknown_properties(node: &KdlNode) -> Result<(), String> {
    let name = node.name().value();
    let allowed = allowed_properties(name);
    for (property, _) in node_properties(node) {
        if !allowed.contains(&property) {
            return Err(format!("unknown property {property:?} on a {name} node"));
        }
    }
    Ok(())
}

fn reject_children(node: &KdlNode) -> Result<(), String> {
    if node.children().is_some() {
        return Err(format!(
            "a {} node must not have a block",
            node.name().value()
        ));
    }
    Ok(())
}

fn child_nodes(node: &KdlNode) -> Vec<&KdlNode> {
    node.children()
        .map(|children| children.nodes().iter().collect())
        .unwrap_or_default()
}

fn node_arguments(node: &KdlNode) -> Vec<&KdlValue> {
    node.entries()
        .iter()
        .filter(|entry| entry.name().is_none())
        .map(|entry| entry.value())
        .collect()
}

fn node_properties(node: &KdlNode) -> Vec<(&str, &KdlValue)> {
    node.entries()
        .iter()
        .filter_map(|entry| entry.name().map(|name| (name.value(), entry.value())))
        .collect()
}

fn property_string<'a>(node: &'a KdlNode, key: &str) -> Result<Option<&'a str>, String> {
    node.get(key)
        .map(|entry| expect_string(entry.value(), &format!("property {key:?}")))
        .transpose()
}

fn required_property_string<'a>(node: &'a KdlNode, key: &str) -> Result<&'a str, String> {
    property_string(node, key)?.ok_or_else(|| {
        format!(
            "a {} node needs a {key:?} property",
            node.name().value()
        )
    })
}

fn property_integer(node: &KdlNode, key: &str) -> Result<Option<i64>, String> {
    node.get(key)
        .map(|entry| {
            entry
                .value()
                .as_i64()
                .ok_or_else(|| format!("property {key:?} must be an integer"))
        })
        .transpose()
}

fn expect_string<'a>(value: &'a KdlValue, what: &str) -> Result<&'a str, String> {
    value
        .as_string()
        .ok_or_else(|| format!("{what} must be a string"))
}

fn single_string_argument(node: &KdlNode) -> Result<&str, String> {
    let name = node.name().value();
    let [value] = node_arguments(node)[..] else {
        return Err(format!("a {name} node takes exactly one argument"));
    };
    expect_string(value, &format!("the {name} argument"))
}

fn single_usize_argument(node: &KdlNode) -> Result<usize, String> {
    let name = node.name().value();
    let [value] = node_arguments(node)[..] else {
        return Err(format!("a {name} node takes exactly one argument"));
    };
    value
        .as_i64()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| format!("{name} must be a non-negative integer"))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
