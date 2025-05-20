use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestSystemMessageArgs,      // system message builder
        ChatCompletionRequestAssistantMessageArgs,   // assistant message builder
        ChatCompletionRequestToolMessageArgs,        // tool response builder
        ChatCompletionMessageToolCall,               // tool-call struct
        FunctionObject,                              // function definition for tool
        ChatCompletionTool,                          // tool struct
        ChatCompletionToolArgs,                      // tool builder
        CreateChatCompletionRequestArgs,             // request builder
        FunctionCall, FunctionName,                  // function-call types
        ChatCompletionToolType,                      // tool types
        ChatCompletionNamedToolChoice,               // tool-choice struct
        ChatCompletionToolChoiceOption,              // tool-choice enum
        Role,                                        // message roles
    },
};
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use log::{error, info};
use question::{Answer, Question};
use rand::seq::SliceRandom;  // for .choose()
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

// CLI definition
#[derive(Parser)]
#[command(version)]
#[command(name = "Auto Commit")]
#[command(author = "Miguel Piedrafita <soy@miguelpiedrafita.com>")]
#[command(about = "Automagically generate commit messages.")]
struct Cli {
    #[clap(flatten)]
    verbose: Verbosity<InfoLevel>,
    #[arg(long = "dry-run", help = "Output the generated message, but don't create a commit.")]
    dry_run: bool,
    #[arg(short, long, help = "Edit the generated commit message before committing.")]
    review: bool,
    #[arg(short, long, help = "Don't ask for confirmation before committing.")]
    force: bool,
}

// Commit schema
#[derive(Debug, serde::Deserialize, JsonSchema)]
struct Commit {
    title: String,
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
    // Parse CLI and init logging
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbose.log_level_filter())
        .init();

    // Ensure API key
    let api_token = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
        error!("Please set the OPENAI_API_KEY environment variable.");
        std::process::exit(1);
    });

    // Gather staged diff
    let git_staged_cmd = Command::new("git")
        .args(["diff", "--staged"])
        .output().map_err(|e| { error!("Failed to get staged diff: {}", e); () })?
        .stdout;
    let git_staged = std::str::from_utf8(&git_staged_cmd).unwrap_or("");
    if git_staged.is_empty() {
        error!("No staged files – try `git add`.");
    }

    // Verify Git repo
    let is_repo = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output().map_err(|e| { error!("Failed repo check: {}", e); () })?
        .stdout;
    if std::str::from_utf8(&is_repo).unwrap_or("") != "true\n" {
        error!("Not in a git repo; run from the root or `git init`.");
        std::process::exit(1);
    }

    // Build OpenAI client
    let client = async_openai::Client::with_config(
        OpenAIConfig::new().with_api_key(api_token),
    );

    // Prepare diff context
    let files = Command::new("git")
        .args(["diff", "--name-only", "--staged"])
        .output().map_err(|e| { error!("Couldn't get file list: {}", e); () })?
        .stdout;
    let files = std::str::from_utf8(&files).unwrap_or("");
    let diff = git_staged; // already UTF-8
    let combined = format!("Changed files:\n{}\n\nDiff:\n{}", files, diff);
    let context = truncate_to_n_tokens(&combined, MAX_DIFF_TOKENS);

    // Optional spinner when silent
    let spinner = if !cli.dry_run && cli.verbose.is_silent() {
        let choices = [
            Spinners::Earth, Spinners::Aesthetic, Spinners::Hearts,
            Spinners::BoxBounce, Spinners::BouncingBar,
            // … add others …
        ];
        Some(Spinner::new(
            *choices[..].choose(&mut rand::thread_rng()).unwrap(),
            "Analyzing code…".into(),
        ))
    } else {
        None
    };

    // Generate JSON schema for Commit
    let mut gen = SchemaGenerator::new(
        SchemaSettings::openapi3().with(|s| s.inline_subschemas = true),
    );
    let commit_schema = gen.subschema_for::<Commit>().into_object();

    // Construct messages
    let messages = vec![
        // System prompt
        ChatCompletionRequestSystemMessageArgs::default()
            .content("You are an experienced developer who writes great commit messages.".to_string())
            .build().unwrap()
            .into(),
        // Assistant invokes get_diff tool
        ChatCompletionRequestAssistantMessageArgs::default()
            .tool_calls(vec![
                ChatCompletionMessageToolCall {
                    id: "call_get_diff".to_string(),
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionCall {
                        name: "get_diff".to_string(),
                        arguments: "{}".to_string(),
                    },
                }
            ])
            .build().unwrap()
            .into(),
        // Tool returns diff
        ChatCompletionRequestToolMessageArgs::default()
            .tool_call_id("call_get_diff".to_string())
            .name("get_diff".to_string())
            .content(context.clone())
            .build().unwrap()
            .into(),
    ];

    // Declare tools
    let tools = vec![
        ChatCompletionToolArgs::default()
            .r#type(ChatCompletionToolType::Function)
            .function(FunctionObject {
                name: "get_diff".to_string(),
                description: Some("Returns the output of `git diff HEAD` as a string.".to_string()),
                parameters: Some(json!({ "type": "object", "properties": {} })),
                strict: None,
            })
.build().expect("Failed to build 'get_diff' tool")
        ChatCompletionToolArgs::default()
            .r#type(ChatCompletionToolType::Function)
            .function(FunctionObject {
                name: "commit".to_string(),
                description: Some("Creates a commit with the given title and a description.".to_string()),
                parameters: Some(serde_json::to_value(commit_schema).unwrap()),
                strict: None,
            })
            .build().unwrap(),
    ];

    // Send request, forcing the "commit" tool
    let completion = client.chat().create(
        CreateChatCompletionRequestArgs::default()
            .model(&get_model_from_env())
            .messages(messages)
            .tools(tools)
            .tool_choice(ChatCompletionToolChoiceOption::Named(
                ChatCompletionNamedToolChoice {
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionName { name: "commit".to_string() },
                }
            ))
            .temperature(0.0)
            .max_tokens(2000u16)
            .build().unwrap()
    ).await.expect("Completion failed");

    // Stop spinner
    if let Some(sp) = spinner {
        sp.stop_with_message("Analysis complete.".into());
    }

    // Parse commit message from the first tool call in the assistant’s response
    let tool_call = &completion.choices[0]
        .message
        .tool_calls.as_ref().unwrap()[0];
    let commit_args_json = &tool_call.function.arguments;
    let commit_msg = serde_json::from_str::<Commit>(commit_args_json)
        .expect("Failed to parse commit JSON")
        .to_string();

    // Dry-run or actual commit
    if cli.dry_run {
        info!("{}", commit_msg);
        return Ok(());
    }
    info!("Proposed Commit:\n{}\n", commit_msg);
    if !cli.force {
        if Question::new("Commit? (Y/n)")
            .yes_no().until_acceptable().default(Answer::YES)
            .ask().unwrap() == Answer::NO
        {
            error!("Aborted.");
            std::process::exit(1);
        }
    }

    // Perform the git commit
    let mut proc_commit = Command::new("git")
        .arg("commit")
        .args(if cli.review { vec!["-e"] } else { vec![] })
        .arg("-F").arg("-")
        .stdin(Stdio::piped())
        .spawn().unwrap();
    let mut stdin = proc_commit.stdin.take().unwrap();
    std::thread::spawn(move || {
        stdin.write_all(commit_msg.as_bytes()).unwrap();
    });
    let out = proc_commit.wait_with_output().unwrap();
    info!("{}", str::from_utf8(&out.stdout).unwrap());

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
        assert!(!cli.dry_run);
        assert!(!cli.review);
        assert!(!cli.force);
        assert_eq!(cli.verbose.log_level_filter(), LevelFilter::Info);
    }

    #[test]
    fn cli_parsing_all_flags_and_verbose_levels() {
        let args = &["auto-commit", "--dry-run", "--review", "--force", "-vv"];
        let cli = Cli::parse_from(args);
        assert!(cli.dry_run);
        assert!(cli.review);
        assert!(cli.force);
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
        assert!(!model.is_empty());
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
