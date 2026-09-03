#![allow(dead_code)]

#[path = "../src/custom_layouts.rs"]
mod custom_layouts;
#[path = "../src/layout_generators.rs"]
mod layout_generators;

use layout_generators::{
    parse_floor_overrides, parse_generator_files, FloorOverrides, GeneratorFile, LayoutGenerator,
    PaneFloors, DEFAULT_MIN_PANE_HEIGHT, DEFAULT_MIN_PANE_WIDTH,
};

/// The generator the README documents, and the shape the run's own layouts use.
pub const MADEV_GENERATOR: &str = r#"
command "impl"
arg "n"
flag "crit-per-impl" value="m" default=2
flag "single-tab"
flag "only-crit" optional-value="from" default=1

tab "{tab}-impl" unless="single_tab only_crit" {
    each for="i" in="1..=n" {
        pane "claude -n impl{i}"
    }
}
each for="k" in="from..from+m" {
    tab "{tab}-crit{k}" unless="single_tab" {
        each for="i" in="1..=n" {
            pane "claude -n impl{i}-crit{k}"
        }
    }
}
tab if="single_tab" {
    each for="i" in="1..=n" {
        pane "claude -n impl{i}"
    }
    each for="k" in="from..from+m" {
        each for="i" in="1..=n" {
            pane "claude -n impl{i}-crit{k}"
        }
    }
}
"#;

fn file(path: &str, content: &str) -> GeneratorFile {
    GeneratorFile {
        path: path.to_string(),
        content: content.to_string(),
    }
}

fn parse_one(content: &str) -> LayoutGenerator {
    let mut generators = parse_generator_files(&[file("/g/impl.kdl", content)])
        .unwrap_or_else(|error| panic!("expected a parse, got {error}"));
    generators.pop().expect("one generator")
}

fn refusal(content: &str) -> String {
    parse_generator_files(&[file("/g/impl.kdl", content)])
        .expect_err("expected a refusal")
}

fn body(nodes: &str) -> String {
    format!("command \"c\"\narg \"n\"\nflag \"only-crit\"\n{nodes}\n")
}

#[test]
fn parses_the_madev_generator() {
    let generator = parse_one(MADEV_GENERATOR);
    assert_eq!(generator.command, "impl");
    assert_eq!(generator.source, "impl.kdl");
}

#[test]
fn keeps_file_order_and_names_both_files_on_a_duplicate_command() {
    let files = [
        file("/g/a.kdl", "command \"impl\"\n"),
        file("/g/b.kdl", "command \"chat\"\n"),
    ];
    let commands: Vec<String> = parse_generator_files(&files)
        .unwrap()
        .into_iter()
        .map(|generator| generator.command)
        .collect();
    assert_eq!(commands, ["impl", "chat"]);

    let clash = [
        file("/g/a.kdl", "command \"impl\"\n"),
        file("/g/b.kdl", "command \"impl\"\n"),
    ];
    let error = parse_generator_files(&clash).unwrap_err();
    assert!(
        error.starts_with("b.kdl:") && error.contains("a.kdl"),
        "{error}"
    );
}

#[test]
fn refuses_an_unknown_node() {
    assert_eq!(
        refusal("command \"c\"\nwindow \"x\"\n"),
        "impl.kdl: unknown node \"window\""
    );
    // A misspelling inside a block is the likeliest typo, and the refusal is the
    // whole diagnostic: no line numbers reach the one-row bar.
    assert_eq!(
        refusal(&body("tab { pnae \"cmd\"; }")),
        "impl.kdl: unknown node \"pnae\""
    );
    assert_eq!(
        refusal(&body("each for=\"i\" in=\"1..2\" { taab { pane \"a\"; }; }")),
        "impl.kdl: unknown node \"taab\""
    );
    assert_eq!(
        refusal(&body("tab { pnae for=\"i\" \"cmd\"; }")),
        "impl.kdl: unknown node \"pnae\""
    );
}

#[test]
fn refuses_an_unknown_property() {
    let error = refusal(&body("tab \"x\" when=\"n\" { pane \"a\"; }"));
    assert!(
        error.contains("unknown property \"when\" on a tab node"),
        "{error}"
    );
}

#[test]
fn refuses_a_file_without_a_command() {
    assert_eq!(
        refusal("arg \"n\"\n"),
        "impl.kdl: a generator file must declare a command"
    );
}

#[test]
fn refuses_a_duplicate_variable() {
    assert!(refusal("command \"c\"\narg \"n\"\narg \"n\"\n").contains("already declared"));
    assert!(refusal("command \"c\"\narg \"tab\"\n").contains("already declared"));
    assert!(
        refusal("command \"c\"\nflag \"single-tab\"\narg \"single_tab\"\n")
            .contains("already declared")
    );
    assert!(
        refusal(&body("each for=\"n\" in=\"1..2\" { tab { pane \"a\"; }; }"))
            .contains("already declared")
    );
}

#[test]
fn refuses_a_declaration_after_the_body() {
    let error = refusal("command \"c\"\ntab { pane \"a\"; }\narg \"n\"\n");
    assert_eq!(
        error,
        "impl.kdl: \"arg\" must be declared before the first tab, pane or each node"
    );
}

#[test]
fn refuses_misplaced_tab_and_pane_nodes() {
    assert!(refusal(&body("tab { tab { pane \"a\"; }; }"))
        .contains("must not be nested inside another tab"));
    assert!(refusal(&body("pane \"a\"")).contains("a pane node must be inside a tab"));
    assert!(refusal(&body("each for=\"i\" in=\"1..2\" { pane \"a\"; }"))
        .contains("a pane node must be inside a tab"));
}

#[test]
fn accepts_a_tab_under_a_top_level_each_chain() {
    parse_one(&body(
        "each for=\"i\" in=\"1..2\" { each for=\"j\" in=\"1..2\" { tab { pane \"a{i}{j}\"; }; }; }",
    ));
}

#[test]
fn refuses_an_optional_value_flag_without_a_default() {
    let error = refusal("command \"c\"\nflag \"only-crit\" optional-value=\"from\"\n");
    assert!(error.contains("needs a default"), "{error}");
    assert!(
        refusal("command \"c\"\nflag \"a\" value=\"v\" optional-value=\"w\" default=1\n")
            .contains("both value and optional-value")
    );
    assert!(refusal("command \"c\"\nflag \"a\" default=1\n").contains("default without a value"));
}

#[test]
fn refuses_a_presence_outside_a_condition() {
    let error = refusal(&body("tab \"{only_crit}\" { pane \"a\"; }"));
    assert!(error.contains("legal only in if or unless"), "{error}");
    assert!(refusal(&body("tab { pane \"a{only_crit}\"; }")).contains("legal only in if or unless"));
    assert!(refusal(&body("each for=\"i\" in=\"1..only_crit\" { tab { pane \"a\"; }; }"))
        .contains("not an integer variable"));
}

#[test]
fn refuses_an_integer_or_the_source_tab_inside_a_condition() {
    assert!(refusal(&body("tab if=\"n\" { pane \"a\"; }")).contains("is not a flag presence"));
    assert!(refusal(&body("tab if=\"tab\" { pane \"a\"; }")).contains("is not a flag presence"));
    assert!(
        refusal(&body("each for=\"i\" in=\"1..tab\" { tab { pane \"a\"; }; }"))
            .contains("not an integer variable")
    );
}

#[test]
fn refuses_an_unknown_variable() {
    assert!(refusal(&body("tab if=\"nope\" { pane \"a\"; }")).contains("unknown variable \"nope\""));
    assert!(
        refusal(&body("each for=\"i\" in=\"1..nope\" { tab { pane \"a\"; }; }"))
            .contains("unknown variable \"nope\"")
    );
}

#[test]
fn refuses_a_malformed_range() {
    assert!(refusal(&body("each for=\"i\" in=\"1\" { tab { pane \"a\"; }; }")).contains("separator"));
    assert!(
        refusal(&body("each for=\"i\" in=\"1..n+1+2\" { tab { pane \"a\"; }; }"))
            .contains("may use at most one")
    );
    assert!(refusal(&body("each for=\"i\" in=\"..n\" { tab { pane \"a\"; }; }"))
        .contains("range bound is empty"));
    assert!(refusal(&body("each for=\"i\" in=\"-1..n\" { tab { pane \"a\"; }; }"))
        .contains("must start with a number or a variable"));
}

#[test]
fn accepts_every_range_shape() {
    parse_one(&body(
        "each for=\"i\" in=\"1..n\" { tab { pane \"a{i}\"; }; }\n\
         each for=\"j\" in=\"1..=n\" { tab { pane \"b{j}\"; }; }\n\
         each for=\"k\" in=\"n+1..=n-1\" { tab { pane \"c{k}\"; }; }\n\
         each for=\"l\" in=\" 2 .. n \" { tab { pane \"d{l}\"; }; }",
    ));
}

#[test]
fn refuses_a_bad_floor_declaration() {
    assert!(refusal("command \"c\"\nmin_pane_width \"wide\"\n")
        .contains("min_pane_width must be a non-negative integer"));
    assert!(refusal("command \"c\"\nmin_pane_height -1\n")
        .contains("min_pane_height must be a non-negative integer"));
    assert!(refusal("command \"c\"\nmin_pane_width 10\nmin_pane_width 20\n")
        .contains("declared more than once"));
}

#[test]
fn refuses_a_malformed_command_or_flag_name() {
    assert!(refusal("command \"two words\"\n").contains("must be a single word"));
    assert!(refusal("command \"c\"\nflag \"-lead\"\n").contains("flag name"));
    assert!(refusal("command \"c\"\nflag \"a b\"\n").contains("flag name"));
}

#[test]
fn refuses_a_block_where_none_belongs() {
    assert!(refusal("command \"c\" { arg \"n\"; }\n").contains("must not have a block"));
    assert!(refusal(&body("tab { pane \"a\" { pane \"b\"; }; }")).contains("must not have a block"));
}

#[test]
fn floors_resolve_from_the_file_then_the_global_then_the_constants() {
    let file_floors = FloorOverrides {
        min_pane_width: Some(80),
        min_pane_height: None,
    };
    let global = FloorOverrides {
        min_pane_width: Some(100),
        min_pane_height: Some(20),
    };
    assert_eq!(
        PaneFloors::resolve(file_floors, global),
        PaneFloors {
            min_pane_width: 80,
            min_pane_height: 20,
        }
    );
    assert_eq!(
        PaneFloors::resolve(FloorOverrides::default(), global),
        PaneFloors {
            min_pane_width: 100,
            min_pane_height: 20,
        }
    );
    assert_eq!(
        PaneFloors::resolve(FloorOverrides::default(), FloorOverrides::default()),
        PaneFloors {
            min_pane_width: DEFAULT_MIN_PANE_WIDTH,
            min_pane_height: DEFAULT_MIN_PANE_HEIGHT,
        }
    );
}

#[test]
fn reads_the_floor_keys_of_zellaude_json() {
    assert_eq!(
        parse_floor_overrides(r#"{"min_pane_width": 100, "min_pane_height": 5}"#).unwrap(),
        FloorOverrides {
            min_pane_width: Some(100),
            min_pane_height: Some(5),
        }
    );
    assert_eq!(
        parse_floor_overrides(r#"{"custom_states": []}"#).unwrap(),
        FloorOverrides::default()
    );
    assert_eq!(parse_floor_overrides("{}").unwrap(), FloorOverrides::default());
    assert_eq!(parse_floor_overrides("[]").unwrap(), FloorOverrides::default());
    assert!(parse_floor_overrides(r#"{"min_pane_width": "wide"}"#)
        .unwrap_err()
        .contains("min_pane_width must be a non-negative integer"));
    assert!(parse_floor_overrides(r#"{"min_pane_height": -1}"#).is_err());
    assert!(parse_floor_overrides("{").is_err());
}
