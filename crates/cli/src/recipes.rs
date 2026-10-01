//! `unisphere prep recipes` / `prep recipe`: named DuckDB research queries over a
//! prep target's canonical views. The CLI only prints SQL; it links and runs no
//! engine and reads nothing from the target.
use std::io::{self, Write};

use serde_json::json;

use crate::{OutputMode, PrepRecipeCommand, PrepRecipesCommand, query::emit_static_error};

struct Recipe {
    name: &'static str,
    /// The consumer-brief or fleet question the recipe answers.
    answers: &'static str,
    sql: &'static str,
}

impl Recipe {
    fn summary(&self) -> &'static str {
        self.sql
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("-- "))
            .unwrap_or_default()
    }

    /// The query with its shared CTE fragments expanded, without a terminator.
    fn query(&self) -> String {
        self.sql
            .replace(
                "{turns_x}",
                &format!("{},\n{}", CALLS_X.trim_end(), TURNS_X.trim_end()),
            )
            .replace("{calls_x}", CALLS_X.trim_end())
            .trim_end()
            .trim_end_matches(';')
            .to_owned()
    }
}

/// `calls_x`: each canonical call with source, session, turn and derived cold flags.
const CALLS_X: &str = include_str!("../recipes/calls_x.sql");
/// `turns_x`: each turn with its calls' token classes; requires `calls_x`.
const TURNS_X: &str = include_str!("../recipes/turns_x.sql");

macro_rules! recipe {
    ($name:literal, $answers:literal) => {
        Recipe {
            name: $name,
            answers: $answers,
            sql: include_str!(concat!("../recipes/", $name, ".sql")),
        }
    };
}

static RECIPES: &[Recipe] = &[
    recipe!("daily", "brief Q1"),
    recipe!("hourly", "brief Q2"),
    recipe!("call-timeline", "brief Q3"),
    recipe!("turn-cost", "brief Q4"),
    recipe!("trigger-kinds", "brief Q5"),
    recipe!("sender-attribution", "brief Q6"),
    recipe!("compactions", "brief Q7"),
    recipe!("context-bands", "brief Q8"),
    recipe!("idle-wakes", "brief Q9"),
    recipe!("limit-events", "brief Q10"),
    recipe!("meter-replay", "brief Q11"),
    recipe!("fan-outs", "brief Q12"),
    recipe!("message-graph", "brief Q13"),
    recipe!("hidden-requests", "brief Q14"),
    recipe!("structure", "brief section 4 structure counts"),
    recipe!("trigger-records", "brief section 4 trigger records by kind"),
    recipe!("idle-tax", "fleet: idle tax"),
    recipe!("compaction-cadence", "fleet: compaction cadence"),
    recipe!("peer-amplification", "fleet: peer amplification"),
    recipe!("subagent-fanout", "fleet: subagent fan-out"),
    recipe!("tool-loops", "fleet: tool loops"),
    recipe!("fleet-idle", "fleet: idle longest"),
    recipe!("expensive-turns", "fleet: most expensive turns"),
    recipe!(
        "daily-tokens",
        "fleet: daily token classes by repo, model and harness"
    ),
];

fn find(name: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|recipe| recipe.name == name)
}

/// List the bundled recipes. Exit 0, or 1 when stdout fails.
pub fn run_prep_recipes(
    command: &PrepRecipesCommand,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    finish(emit_list(command.mode, stdout), 0, stderr)
}

fn emit_list(mode: OutputMode, stdout: &mut dyn Write) -> io::Result<()> {
    match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            let recipes = RECIPES
                .iter()
                .map(|recipe| json!({"name": recipe.name, "answers": recipe.answers, "summary": recipe.summary()}))
                .collect::<Vec<_>>();
            serde_json::to_writer(
                &mut *stdout,
                &json!({"ok": true, "command": "prep.recipes", "v": 1,
                    "data": {"engine": "duckdb", "recipes": recipes},
                    "next_action": {"summary": "Print one recipe and pipe it into an installed DuckDB CLI: `unisphere prep recipe NAME --target DIR | duckdb`.",
                        "argv": ["unisphere", "prep", "recipe"], "required_inputs": ["name", "target"]}}),
            )
            .map_err(io::Error::other)?;
            stdout.write_all(b"\n")?;
        }
        OutputMode::Human => {
            writeln!(stdout, "Bundled DuckDB research recipes:")?;
            for recipe in RECIPES {
                writeln!(
                    stdout,
                    "  {} — {} ({})",
                    recipe.name,
                    recipe.summary(),
                    recipe.answers
                )?;
            }
            writeln!(
                stdout,
                "Next: run `unisphere prep recipe NAME --target DIR | duckdb`; see `unisphere docs get research-recipes --human`."
            )?;
        }
    }
    stdout.flush()
}

/// Print one recipe as a self-contained DuckDB script over `TARGET/views.sql`.
/// The script is written to stdout in every output mode so it can be piped into
/// `duckdb`; diagnostics go to stderr. Exit 2 for an unknown recipe or a target
/// path that cannot be quoted in the script, 1 when stdout fails.
pub fn run_prep_recipe(
    command: &PrepRecipeCommand,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let Some(recipe) = find(&command.name) else {
        let names = RECIPES.iter().map(|recipe| recipe.name).collect::<Vec<_>>();
        return refuse(
            "UNI-CLI-PREP-RECIPE",
            "The requested research recipe is not bundled.",
            "Run `unisphere prep recipes --json` and choose one of the returned names.",
            &names,
            command.mode,
            stderr,
        );
    };
    let Some(target) = command
        .target
        .to_str()
        .filter(|target| !target.contains('\'') && !target.chars().any(char::is_control))
    else {
        return refuse(
            "UNI-CLI-PREP-TARGET",
            "The target path cannot be quoted in a DuckDB script: it is not UTF-8 or contains a single quote or control character.",
            "Move or link the prep target to a plain UTF-8 path without quotes, then re-run with that --target.",
            &[],
            command.mode,
            stderr,
        );
    };
    let written = write!(
        stdout,
        "SET file_search_path = '{target}';\n.read '{target}/views.sql'\n{};\n",
        recipe.query()
    )
    .and_then(|()| stdout.flush());
    if written.is_ok() && command.mode == OutputMode::Human {
        let _ = writeln!(
            stderr,
            "Next: pipe this script into DuckDB: `unisphere prep recipe {} --target DIR | duckdb`.",
            recipe.name
        );
    }
    finish(written, 0, stderr)
}

fn refuse(
    code: &str,
    message: &str,
    action: &str,
    alternatives: &[&str],
    mode: OutputMode,
    stderr: &mut dyn Write,
) -> u8 {
    let written = emit_static_error(
        "prep.recipe",
        code,
        message,
        action,
        alternatives,
        mode,
        stderr,
    );
    if written.is_ok() { 2 } else { 1 }
}

fn finish(written: io::Result<()>, exit: u8, stderr: &mut dyn Write) -> u8 {
    if written.is_ok() {
        return exit;
    }
    let _ = stderr
        .write_all(b"unisphere: output incomplete; choose a healthy destination and retry.\n");
    let _ = stderr.flush();
    1
}
