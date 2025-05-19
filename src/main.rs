use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionFunctionCall, ChatCompletionFunctions, ChatCompletionRequestMessage,
        CreateChatCompletionRequestArgs, FunctionCall, Role,
    },
};
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use log::{error, info};
use question::{Answer, Question};
use rand::seq::SliceRandom;
use schemars::{
    gen::{SchemaGenerator, SchemaSettings},
    JsonSchema,
};
use serde_json::json;
use spinners::{Spinner, Spinners};
use std::{
    io::Write,
    process::{Command, Stdio},
    str,
};
use auto_commit::{get_model_from_env, truncate_to_n_tokens};

#[derive(Parser)]
#[command(version)]
#[command(name = "Auto Commit")]
#[command(author = "Miguel Piedrafita <soy@miguelpiedrafita.com>")]
#[command(about = "Automagically generate commit messages.", long_about = None)]
struct Cli {
    #[clap(flatten)]
    verbose: Verbosity<InfoLevel>,

    #[arg(
        long = "dry-run",
        help = "Output the generated message, but don't create a commit."
    )]
    dry_run: bool,

    #[arg(
        short,
        long,
        help = "Edit the generated commit message before committing."
    )]
    review: bool,

    #[arg(short, long, help = "Don't ask for confirmation before committing.")]
    force: bool,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct Commit {
    /// The title of the commit.
    title: String,

    /// An exhaustive description of the changes.
    description: String,
}

impl ToString for Commit {
    fn to_string(&self) -> String {
        format!("{}\n\n{}", self.title, self.description)
    }
}

const MAX_DIFF_TOKENS: usize = 20_000;

#[tokio::main]
async fn main() -> Result<(), ()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbose.log_level_filter())
        .init();

    let api_token = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
        error!("Please set the OPENAI_API_KEY environment variable.");
        std::process::exit(1);
    });

    let git_staged_cmd = Command::new("git")
        .arg("diff")
        .arg("--staged")
        .output()
        .map_err(|e| {
            error!("Failed to get staged diff: {}", e);
            ()
        })?
        .stdout;

    let git_staged_cmd = match str::from_utf8(&git_staged_cmd) {
        Ok(v) => v,
        Err(e) => {
            error!("Staged diff output was not valid UTF-8: {}", e);
            ""
        }
    };

    if git_staged_cmd.is_empty() {
        error!("There are no staged files to commit.\nTry running `git add` to stage some files.");
    }

    let is_repo = Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .map_err(|e| {
            error!("Failed to check if this is a git repository: {}", e);
            ()
        })?
        .stdout;

    if str::from_utf8(&is_repo).unwrap_or("").trim() != "true" {
        error!("It looks like you are not in a git repository.\nPlease run this command from the root of a git repository, or initialize one using `git init`.");
        std::process::exit(1);
    }

    let client = async_openai::Client::with_config(OpenAIConfig::new().with_api_key(api_token));

    let files_output = Command::new("git")
        .arg("diff")
        .arg("--name-only")
        .arg("--staged")
        .output()
        .map_err(|e| {
            error!("Couldn't get changed files: {}", e);
            ()
        })?
        .stdout;
    let files_changed = match str::from_utf8(&files_output) {
        Ok(v) => v,
        Err(e) => {
            error!("Changed files output was not valid UTF-8: {}", e);
            ""
        }
    };

    let diff_output = Command::new("git")
        .arg("diff")
        .arg("--staged")
        .output()
        .map_err(|e| {
            error!("Couldn't find diff: {}", e);
            ()
        })?
        .stdout;
    let diff_output = match str::from_utf8(&diff_output) {
        Ok(v) => v,
        Err(e) => {
            error!("Diff output was not valid UTF-8: {}", e);
            ""
        }
    };

    let combined = format!("Changed files:\n{}\n\nDiff:\n{}", files_changed, diff_output);
    let output = truncate_to_n_tokens(&combined, MAX_DIFF_TOKENS);

    if !cli.dry_run {
        info!("Loading Data...");
    }

    let sp: Option<Spinner> = if !cli.dry_run && cli.verbose.is_silent() {
        let vs = [
            Spinners::Earth,
            Spinners::Aesthetic,
            Spinners::Hearts,
            Spinners::BoxBounce,
            Spinners::BoxBounce2,
            Spinners::BouncingBar,
            Spinners::Christmas,
            Spinners::Clock,
            Spinners::FingerDance,
            Spinners::FistBump,
            Spinners::Flip,
            Spinners::Layer,
            Spinners::Line,
            Spinners::Material,
            Spinners::Mindblown,
            Spinners::Monkey,
            Spinners::Noise,
            Spinners::Point,
            Spinners::Pong,
            Spinners::Runner,
            Spinners::SoccerHeader,
            Spinners::Speaker,
            Spinners::SquareCorners,
            Spinners::Triangle,
        ];

        let spinner = vs.choose(&mut rand::thread_rng()).unwrap().clone();

        Some(Spinner::new(spinner, "Analyzing Codebase...".into()))
    } else {
        None
    };

    let mut generator = SchemaGenerator::new(SchemaSettings::openapi3().with(|settings| {
        settings.inline_subschemas = true;
    }));

    let commit_schema = generator.subschema_for::<Commit>().into_object();

    let completion = client
        .chat()
        .create(
            CreateChatCompletionRequestArgs::default()
                .messages(vec![
                    ChatCompletionRequestMessage {
                        role: Role::System,
                        content: Some(
                            "You are an experienced developer who writes great commit messages."
                                .to_string(),
                        ),
                        ..Default::default()
                    },
                    ChatCompletionRequestMessage {
                        role: Role::Assistant,
                        content: Some("".to_string()),
                        function_call: Some(FunctionCall {
                            arguments: "{}".to_string(),
                            name: "get_diff".to_string(),
                        }),
                        ..Default::default()
                    },
                    ChatCompletionRequestMessage {
                        role: Role::Function,
                        content: Some(output.to_string()),
                        name: Some("get_diff".to_string()),
                        ..Default::default()
                    },
                ])
                .functions(vec![
                    ChatCompletionFunctions {
                        name: "get_diff".to_string(),
                        description: Some(
                            "Returns the output of `git diff HEAD` as a string.".to_string(),
                        ),
                        parameters: Some(json!({
                            "type": "object",
                            "properties": {}
                        })),
                    },
                    ChatCompletionFunctions {
                        name: "commit".to_string(),
                        description: Some(
                            "Creates a commit with the given title and a description.".to_string(),
                        ),
                        parameters: Some(serde_json::to_value(commit_schema).unwrap()),
                    },
                ])
                .function_call(ChatCompletionFunctionCall::Object(
                    json!({ "name": "commit" }),
                ))
                .model(&get_model_from_env())
                .temperature(0.0)
                .max_tokens(2000u16)
                .build()
                .unwrap(),
        )
        .await
        .expect("Couldn't complete prompt.");

    if sp.is_some() {
        sp.unwrap().stop_with_message("Finished Analyzing!".into());
    }

    let commit_data = &completion.choices[0].message.function_call;
    let commit_msg = serde_json::from_str::<Commit>(&commit_data.as_ref().unwrap().arguments)
        .expect("Couldn't parse model response.")
        .to_string();

    if cli.dry_run {
        info!("{}", commit_msg);
        return Ok(());
    } else {
        info!(
            "Proposed Commit:\n------------------------------\n{}\n------------------------------",
            commit_msg
        );

        if !cli.force {
            let answer = Question::new("Do you want to continue? (Y/n)")
                .yes_no()
                .until_acceptable()
                .default(Answer::YES)
                .ask()
                .expect("Couldn't ask question.");

            if answer == Answer::NO {
                error!("Commit aborted by user.");
                std::process::exit(1);
            }
            info!("Committing Message...");
        }
    }

    let mut ps_commit = Command::new("git")
        .arg("commit")
        .args(if cli.review { vec!["-e"] } else { vec![] })
        .arg("-F")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = ps_commit.stdin.take().expect("Failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all(commit_msg.as_bytes())
            .expect("Failed to write to stdin");
    });

    let commit_output = ps_commit
        .wait_with_output()
        .expect("There was an error when creating the commit.");

    info!("{}", str::from_utf8(&commit_output.stdout).unwrap());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_verbosity_flag::{InfoLevel, Verbosity};
    use log::LevelFilter;

    #[test]
    fn commit_to_string_formats_title_and_description() {
        let commit = Commit {
            title: "Fix bug".to_string(),
            description: "Detailed description".to_string(),
        };
        assert_eq!(commit.to_string(), "Fix bug\n\nDetailed description");
    }

    #[test]
    fn cli_default_parsing_sets_flags_and_info_level() {
        let cli = Cli::parse_from(&["auto-commit"]);
        assert!(!cli.dry_run, "dry_run should be false by default");
        assert!(!cli.review, "review should be false by default");
        assert!(!cli.force, "force should be false by default");
        assert_eq!(cli.verbose.log_level_filter(), LevelFilter::Info);
    }

    #[test]
    fn cli_parsing_all_flags_and_verbose_levels() {
        let args = &["auto-commit", "--dry-run", "--review", "--force", "-vv"];
        let cli = Cli::parse_from(args);
        assert!(cli.dry_run, "dry_run should be true when --dry-run is passed");
        assert!(cli.review, "review should be true when --review is passed");
        assert!(cli.force, "force should be true when --force is passed");
        assert_eq!(cli.verbose.log_level_filter(), LevelFilter::Debug);
    }

    #[test]
    fn get_model_from_env_returns_env_value_when_set() {
        std::env::set_var("AUTO_COMMIT_MODEL", "test-model");
        let model = get_model_from_env();
        assert_eq!(model, "test-model".to_string());
        std::env::remove_var("AUTO_COMMIT_MODEL");
    }

    #[test]
    fn get_model_from_env_returns_non_empty_default_when_unset() {
        std::env::remove_var("AUTO_COMMIT_MODEL");
        let model = get_model_from_env();
        assert!(!model.is_empty(), "Default model should not be empty");
    }

    #[test]
    fn truncate_to_n_tokens_returns_original_when_under_limit() {
        let input = "one two three";
        let result = truncate_to_n_tokens(input, 5);
        assert_eq!(result, input.to_string());
    }

    #[test]
    fn truncate_to_n_tokens_truncates_to_specified_token_count() {
        let input = (1..=10).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
        let result = truncate_to_n_tokens(&input, 5);
        assert_eq!(result.split_whitespace().count(), 5);
    }
}