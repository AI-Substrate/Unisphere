//! Research recipe catalogue: list envelopes, self-contained DuckDB scripts over
//! the published views, refusals, and the bundled research-recipes topic.
use std::{ffi::OsString, path::PathBuf};

use serde_json::Value;
use unisphere_cli::{
    CliContext, OutputMode, ParsedCommand, PrepRecipeCommand, parse, run_docs, run_prep_recipe,
    run_prep_recipes,
};

const TOPIC: &str = include_str!("../docs/research-recipes.md");

fn context() -> CliContext {
    CliContext {
        cwd: PathBuf::from("/work"),
        stdout_is_terminal: false,
        version: "recipes-test".into(),
    }
}

fn parsed(values: &[&str]) -> ParsedCommand {
    let args = values.iter().map(OsString::from).collect();
    match parse(args, &context()) {
        Ok(command) => command,
        Err(failure) => panic!("{values:?} did not parse: {}", failure.code()),
    }
}

fn recipe_command(values: &[&str]) -> PrepRecipeCommand {
    match parsed(values) {
        ParsedCommand::PrepRecipe(command) => command,
        _ => panic!("{values:?} is not a recipe command"),
    }
}

fn listed() -> Vec<Value> {
    let ParsedCommand::PrepRecipes(command) = parsed(&["unisphere", "prep", "recipes", "--json"])
    else {
        panic!("recipes route")
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert_eq!(run_prep_recipes(&command, &mut stdout, &mut stderr), 0);
    assert!(stderr.is_empty());
    let value: Value = serde_json::from_slice(&stdout).expect("one JSON envelope");
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "prep.recipes");
    assert_eq!(value["data"]["engine"], "duckdb");
    assert_eq!(
        value["next_action"]["required_inputs"],
        serde_json::json!(["name", "target"])
    );
    value["data"]["recipes"]
        .as_array()
        .expect("recipes")
        .clone()
}

fn names() -> Vec<String> {
    listed()
        .iter()
        .map(|recipe| recipe["name"].as_str().expect("name").to_owned())
        .collect()
}

fn script(name: &str, mode_flag: &str) -> (u8, String, String) {
    let command = recipe_command(&[
        "unisphere",
        "prep",
        "recipe",
        name,
        "--target",
        "prep/./study",
        mode_flag,
    ]);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let exit = run_prep_recipe(&command, &mut stdout, &mut stderr);
    (
        exit,
        String::from_utf8(stdout).expect("utf-8 script"),
        String::from_utf8(stderr).expect("utf-8 diagnostics"),
    )
}

#[test]
fn catalogue_answers_every_brief_and_fleet_question() {
    let recipes = listed();
    let answers = recipes
        .iter()
        .map(|recipe| recipe["answers"].as_str().expect("answers"))
        .collect::<Vec<_>>();
    for question in 1..=14 {
        let tag = format!("brief Q{question}");
        assert_eq!(
            answers.iter().filter(|answer| **answer == tag).count(),
            1,
            "{tag} is answered by exactly one recipe"
        );
    }
    for fleet in [
        "idle tax",
        "compaction cadence",
        "peer amplification",
        "subagent fan-out",
        "tool loops",
        "idle longest",
        "most expensive turns",
        "daily token classes by repo, model and harness",
    ] {
        assert!(
            answers.contains(&format!("fleet: {fleet}").as_str()),
            "fleet question {fleet:?} has a recipe"
        );
    }
    let mut unique = names();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), recipes.len(), "recipe names are unique");
    for recipe in &recipes {
        let summary = recipe["summary"].as_str().expect("summary");
        assert!(
            !summary.is_empty() && summary.ends_with('.'),
            "{recipe}: one-line summary"
        );
    }
}

#[test]
fn human_list_names_every_recipe_and_the_pipe() {
    let ParsedCommand::PrepRecipes(command) = parsed(&["unisphere", "prep", "recipes", "--human"])
    else {
        panic!("recipes route")
    };
    assert_eq!(command.mode, OutputMode::Human);
    let mut stdout = Vec::new();
    assert_eq!(run_prep_recipes(&command, &mut stdout, &mut Vec::new()), 0);
    let text = String::from_utf8(stdout).unwrap();
    for name in names() {
        assert!(text.contains(&format!("  {name} — ")), "{name} listed");
    }
    assert!(text.contains("| duckdb"));
}

#[test]
fn every_recipe_prints_a_self_contained_script_over_the_views() {
    for name in names() {
        for flag in ["--json", "--human"] {
            let (exit, script, stderr) = script(&name, flag);
            assert_eq!(exit, 0, "{name}");
            let mut lines = script.lines();
            assert_eq!(
                lines.next(),
                Some("SET file_search_path = '/work/prep/study';"),
                "{name}: target is absolute and normalised"
            );
            assert_eq!(lines.next(), Some(".read '/work/prep/study/views.sql'"));
            let query = lines.collect::<Vec<_>>().join("\n");
            assert!(script.ends_with(";\n"), "{name}: terminated");
            assert_eq!(
                query.matches(';').count(),
                1,
                "{name}: exactly one statement after .read"
            );
            assert!(
                !query.contains('{') && !query.contains('}'),
                "{name}: every fragment is expanded"
            );
            assert!(
                !query.contains("read_parquet") && !query.contains("tables/"),
                "{name}: reads only the published views"
            );
            assert!(query.contains("_v"), "{name}: reads a canonical view");
            if flag == "--json" {
                assert!(stderr.is_empty(), "{name}: no JSON-mode chatter");
            } else {
                assert!(stderr.starts_with("Next: "), "{name}: human hint on stderr");
            }
        }
    }
}

#[test]
fn unknown_recipe_exits_2_with_the_valid_names_and_no_script() {
    for (flag, json) in [("--json", true), ("--human", false)] {
        let command = recipe_command(&[
            "unisphere",
            "prep",
            "recipe",
            "no-such-recipe",
            "--target",
            "out",
            flag,
        ]);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        assert_eq!(run_prep_recipe(&command, &mut stdout, &mut stderr), 2);
        assert!(stdout.is_empty(), "nothing reaches the engine pipe");
        if json {
            let value: Value = serde_json::from_slice(&stderr).expect("JSON diagnostic");
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"]["code"], "UNI-CLI-PREP-RECIPE");
            let alternatives = value["error"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(alternatives, names());
        } else {
            let text = String::from_utf8(stderr).unwrap();
            assert!(text.starts_with("UNI-CLI-PREP-RECIPE: "));
            assert!(text.contains("Valid alternatives: daily, hourly,"));
        }
    }
}

#[test]
fn a_target_that_cannot_be_quoted_is_refused() {
    let command = recipe_command(&[
        "unisphere",
        "prep",
        "recipe",
        "daily",
        "--target",
        "/data/o'brien",
        "--json",
    ]);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert_eq!(run_prep_recipe(&command, &mut stdout, &mut stderr), 2);
    assert!(stdout.is_empty());
    let value: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(value["error"]["code"], "UNI-CLI-PREP-TARGET");
}

#[test]
fn recipe_requires_a_name_and_a_target() {
    for values in [
        &["unisphere", "prep", "recipe", "--target", "out"][..],
        &["unisphere", "prep", "recipe", "daily"][..],
    ] {
        let args = values.iter().map(OsString::from).collect();
        assert!(parse(args, &context()).is_err(), "{values:?} parsed");
    }
}

/// The `unisphere …` argv of each ```sh line, up to the first `|` or `>`.
fn documented_examples(markdown: &str) -> Vec<Vec<OsString>> {
    let mut examples = Vec::new();
    let mut in_sh = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_sh = trimmed == "```sh";
            continue;
        }
        if in_sh && trimmed.starts_with("unisphere ") {
            let bound = trimmed.replace("$TARGET", "/fixtures/prep-target");
            let command = bound
                .split_whitespace()
                .take_while(|word| *word != "|" && *word != ">")
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert!(
                !command
                    .iter()
                    .any(|word| word.to_string_lossy().contains('$')),
                "unbound placeholder in {trimmed:?}"
            );
            examples.push(command);
        }
    }
    examples
}

#[test]
fn research_recipes_topic_is_registered_and_complete() {
    let ParsedCommand::Docs(docs) =
        parsed(&["unisphere", "docs", "get", "research-recipes", "--json"])
    else {
        panic!("docs route")
    };
    let mut stdout = Vec::new();
    assert_eq!(run_docs(&docs, &mut stdout, &mut Vec::new()), 0);
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["data"]["id"], "research-recipes");
    assert_eq!(value["data"]["text"], TOPIC);

    assert!(TOPIC.contains("unisphere prep recipe daily --target $TARGET | duckdb"));
    assert!(TOPIC.contains("duckdb: command not found"));
    for name in names() {
        assert!(
            TOPIC.contains(&format!("| `{name}` |")),
            "{name} is in the topic's recipe table"
        );
    }

    let examples = documented_examples(TOPIC);
    assert!(examples.len() >= 8, "{} examples", examples.len());
    for example in examples {
        assert!(
            parse(example.clone(), &context()).is_ok(),
            "example does not parse: {example:?}"
        );
    }
}
