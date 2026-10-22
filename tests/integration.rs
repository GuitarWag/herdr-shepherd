mod support;

use serde_json::{Value, json};
use std::{
    env, fs,
    os::{fd::AsRawFd, unix::fs::symlink},
    path::Path,
    thread,
    time::Duration,
};
use support::*;

fn models(launches: &[Vec<String>]) -> Vec<String> {
    launches
        .iter()
        .map(|args| args[args.iter().position(|arg| arg == "--model").unwrap() + 1].clone())
        .collect()
}

fn install_is_repeatable_and_registers_both_agents() {
    let f = Fixture::new();
    for args in [vec![], vec![], vec!["check"]] {
        status(&f.install(&args), 0);
    }
    for name in ["herdr-shepherd", "herd-boss", "herd-view"] {
        assert!(f.home.join(".local/bin").join(name).is_symlink());
    }
    for directory in [".claude", ".agents"] {
        assert_eq!(
            f.home
                .join(directory)
                .join("skills/herdr-shepherd")
                .canonicalize()
                .unwrap(),
            f.package.join("skills/herdr-shepherd")
        );
    }
    assert!(!f.home.join(".zshrc").exists());
    assert!(!f.home.join(".config").exists());
}

fn existing_command_stops_install_before_any_new_links() {
    let f = Fixture::new();
    let target = f.home.join(".local/bin/herd-boss");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "keep this command").unwrap();
    let result = f.install(&[]);
    status(&result, 1);
    assert!(stderr(&result).contains("Preserve the existing path"));
    assert_eq!(fs::read_to_string(target).unwrap(), "keep this command");
    assert!(!f.home.join(".local/bin/herdr-shepherd").exists());
    assert!(!f.home.join(".agents").exists());
}

fn existing_skill_directory_is_preserved() {
    let f = Fixture::new();
    let target = f.home.join(".agents/skills/herdr-shepherd");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("SKILL.md"), "keep this skill").unwrap();
    status(&f.install(&[]), 1);
    assert_eq!(
        fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "keep this skill"
    );
    assert!(!f.home.join(".local").exists());
}

fn stale_link_is_preserved() {
    let f = Fixture::new();
    let target = f.home.join(".local/bin/herdr-shepherd");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let source = f.root.join("missing-command");
    symlink(&source, &target).unwrap();
    status(&f.install(&[]), 1);
    assert_eq!(fs::read_link(&target).unwrap(), source);
    assert!(!target.parent().unwrap().join("herd-boss").exists());
}

fn missing_agent_stops_install_before_writes() {
    let f = Fixture::new();
    for agent in ["codex", "claude"] {
        fs::remove_file(f.bin.join(agent)).unwrap();
    }
    let result = f.install(&[]);
    status(&result, 1);
    assert!(stderr(&result).contains("Install Claude Code or Codex"));
    assert!(!f.home.join(".local").exists());
}

fn unsupported_herdr_stops_install_before_writes() {
    let mut f = Fixture::new();
    f.env.insert("HERD_TEST_VERSION".into(), "0.7.2".into());
    let result = f.install(&[]);
    status(&result, 1);
    assert!(stderr(&result).contains("herdr 0.7.3 or later"));
    assert!(!f.home.join(".local").exists());
}

fn check_mode_does_not_install_missing_links() {
    let f = Fixture::new();
    let result = f.install(&["check"]);
    status(&result, 1);
    assert!(stderr(&result).contains("The link is missing"));
    for directory in [".local", ".agents", ".claude"] {
        assert!(!f.home.join(directory).exists());
    }
}

fn relocated_checkout_installs_and_runs_without_original_paths() {
    let mut f = Fixture::new();
    let original = f.package.clone();
    f.package = f.root.join("standalone checkout");
    copy_package(&f.package);
    fs::remove_dir_all(&original).unwrap();
    status(&f.install(&[]), 0);
    let launcher = f.home.join(".local/bin/herdr-shepherd");
    assert_eq!(
        launcher.canonicalize().unwrap(),
        f.package.join("bin/herdr-shepherd")
    );
    let result = output(
        f.command(
            launcher,
            &[
                "test",
                "--cwd",
                f.home.to_str().unwrap(),
                "--boss-agent",
                "codex",
                "--worker-agent",
                "codex",
                "--workers",
                "1",
                "--dry-run",
            ],
        ),
        "",
    );
    status(&result, 0);
    assert!(
        stdout(&result).contains(
            f.package
                .join("skills/herdr-shepherd/SKILL.md")
                .to_str()
                .unwrap()
        )
    );
    assert!(!stdout(&result).contains(original.to_str().unwrap()));
}

fn interactive_defaults_preview_without_external_commands() {
    let mut f = Fixture::new();
    f.env.insert(
        "PATH".into(),
        f.root.join("empty-path").to_str().unwrap().into(),
    );
    let result = f.input(&["--interactive", "--dry-run"], "\n\n\n\n\n");
    status(&result, 0);
    let text = stdout(&result);
    for expected in [
        "Command: herdr-shepherd my-task",
        "--boss-agent claude --worker-agent claude --workers 3",
        "--model fable",
        "--model opus",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(f.calls().is_empty());
}

fn interactive_choices_use_the_existing_launch_path() {
    let f = Fixture::new();
    let folder = f.root.join("project 'with' spaces");
    fs::create_dir(&folder).unwrap();
    status(
        &f.input(
            &["--interactive"],
            &format!("interactive-test\n{}\n2\nclaude\n1\ny\n", folder.display()),
        ),
        0,
    );
    let launches = f.launches();
    assert_eq!(
        launches
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["codex", "claude"]
    );
    assert_eq!(models(&launches), ["gpt-6-astra", "opus"]);
    for call in f.calls() {
        if let Some(index) = call.iter().position(|arg| arg == "--cwd") {
            assert_eq!(call[index + 1], folder.to_str().unwrap());
        }
    }
}

fn interactive_worker_list_uses_each_entry() {
    let f = Fixture::new();
    let result = f.input(
        &["--interactive", "--dry-run"],
        "\n\ncodex:boss-model\ncodex:m1,claude\n\n",
    );
    status(&result, 0);
    let text = stdout(&result);
    for expected in [
        "--boss-agent codex:boss-model --worker-agent codex:m1 --worker-agent claude --workers 2",
        "--model boss-model",
        "--model m1",
        "--model opus",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(f.calls().is_empty());
}

fn interactive_expands_home_and_allows_zero_workers() {
    let f = Fixture::new();
    let folder = f.home.join("project");
    fs::create_dir(&folder).unwrap();
    let result = f.input(
        &["--interactive", "--dry-run"],
        "\n~/project\ncodex\n2\n0\n",
    );
    status(&result, 0);
    let text = stdout(&result);
    assert!(text.contains(&format!("--cwd {}", folder.display())));
    assert!(text.contains("--workers 0"));
    assert_eq!(text.matches("pane send-text").count(), 1);
    assert!(f.calls().is_empty());
}

fn interactive_help_does_not_prompt() {
    let f = Fixture::new();
    let result = f.run(&["--interactive", "-h"]);
    status(&result, 0);
    assert!(stdout(&result).contains("--interactive [--dry-run]"));
    assert!(!stdout(&result).contains("Task name ["));
    assert!(f.calls().is_empty());
}

fn interactive_cancel_eof_and_conflicting_options_create_no_panes() {
    let f = Fixture::new();
    for (args, answer, expected) in [
        (vec!["--interactive"], "\n\n\n\n\nn\n", 0),
        (vec!["--interactive"], "", 2),
        (vec!["--interactive"], "task\n", 2),
        (vec!["--interactive", "--team"], "", 2),
        (vec!["task", "--interactive"], "", 2),
    ] {
        status(&f.input(&args, answer), expected);
        assert!(f.calls().is_empty());
    }
}

fn no_arguments_open_ratatui_and_cancel_restores_the_terminal() {
    let f = Fixture::new();
    for key in [b"\x1b".as_slice(), b"\x03".as_slice()] {
        let (code, text) = terminal(&f, &[], key, (28, 100));
        assert_eq!(code, 0, "{text}");
        for expected in ["Task", "Setup cancelled.", "\x1b[?1049l"] {
            assert!(text.contains(expected), "{text}");
        }
        assert!(f.calls().is_empty());
    }
}

fn ratatui_preview_uses_choices_and_restores_the_terminal() {
    let f = Fixture::new();
    let (code, text) = terminal(
        &f,
        &["--interactive", "--dry-run"],
        b"tui-team\r\r\x1b[C\r\x1b[C\r0\r\r",
        (28, 100),
    );
    assert_eq!(code, 0, "{text}");
    for expected in ["gpt-6-astra", "tui-team-boss", "\x1b[?1049l"] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(f.calls().is_empty());
}

fn ratatui_restores_the_terminal_before_a_launch_error() {
    let mut f = Fixture::new();
    f.env.insert("HERD_TEST_ERROR".into(), "tab create".into());
    let (code, text) = terminal(&f, &[], b"\r\r\r\r0\r\r", (28, 100));
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("requested operation failed"));
    assert!(text.contains("\x1b[?1049l"));
}

fn small_terminal_can_cancel_without_creating_a_team() {
    let f = Fixture::new();
    let (code, text) = terminal(&f, &[], b"\r\r\x1b", (10, 40));
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Resize"));
    assert!(f.calls().is_empty());
}

fn default_launch_preserves_claude_models_and_names() {
    let f = Fixture::new();
    status(&f.run(&["test"]), 0);
    let launches = f.launches();
    assert_eq!(
        launches
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["claude"; 4]
    );
    assert_eq!(models(&launches), ["fable", "opus", "opus", "opus"]);
    let names = ["test-boss", "test-w1", "test-w2", "test-w3"];
    assert_eq!(
        launches
            .iter()
            .map(|args| args[2].as_str())
            .collect::<Vec<_>>(),
        names
    );
    assert_eq!(
        f.calls()
            .iter()
            .filter(|call| call[..2] == ["agent", "rename"])
            .map(|call| call[3].as_str())
            .collect::<Vec<_>>(),
        names
    );
    for args in launches {
        assert!(
            args.last()
                .unwrap()
                .contains("Read the herdr-shepherd skill at")
        );
    }
}

fn each_boss_type_supports_mixed_workers() {
    let f = Fixture::new();
    for boss in ["claude", "codex"] {
        f.clear_calls();
        status(
            &f.run(&[
                "mixed",
                "--boss-agent",
                boss,
                "--worker-agent",
                "codex",
                "--worker-agent",
                "claude",
            ]),
            0,
        );
        let launches = f.launches();
        assert_eq!(
            launches
                .iter()
                .map(|args| args[0].as_str())
                .collect::<Vec<_>>(),
            [boss, "codex", "claude"]
        );
        for (role, args) in launches.iter().enumerate() {
            if args[0] == "codex" {
                assert_eq!(
                    models(std::slice::from_ref(args))[0],
                    if role == 0 {
                        "gpt-6-astra"
                    } else {
                        "gpt-6.1-sol"
                    }
                );
                assert!(!args.contains(&"--name".into()));
                assert!(args.contains(&"--approve-for-me".into()));
            }
        }
        assert!(
            launches[1]
                .last()
                .unwrap()
                .contains("Your boss is mixed-boss.")
        );
        assert_eq!(
            f.calls()
                .iter()
                .filter(|call| call[..2] == ["pane", "send-keys"])
                .count(),
            3
        );
    }
}

fn single_worker_type_applies_to_the_worker_count() {
    let f = Fixture::new();
    status(
        &f.run(&[
            "test",
            "--boss-agent",
            "codex",
            "--worker-agent",
            "codex",
            "--workers",
            "02",
        ]),
        0,
    );
    let launches = f.launches();
    assert_eq!(
        launches
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["codex"; 3]
    );
    assert_eq!(
        models(&launches),
        ["gpt-6-astra", "gpt-6.1-sol", "gpt-6.1-sol"]
    );
}

fn codex_model_options_override_role_defaults() {
    let f = Fixture::new();
    for (extra, expected) in [
        (
            vec![
                "--boss-model",
                "boss-model",
                "--worker-model",
                "worker-model",
            ],
            ["boss-model", "worker-model"],
        ),
        (vec!["--model", "shared-model"], ["shared-model"; 2]),
    ] {
        f.clear_calls();
        let mut args = vec![
            "test",
            "--boss-agent",
            "codex",
            "--worker-agent",
            "codex",
            "--workers",
            "1",
        ];
        args.extend(extra);
        status(&f.run(&args), 0);
        assert_eq!(models(&f.launches()), expected);
    }
}

fn agent_entries_select_models_in_a_mixed_list() {
    let f = Fixture::new();
    status(
        &f.run(&[
            "test",
            "--boss-agent",
            "codex:boss-entry",
            "--worker-agent",
            "codex:gpt-6.1-sol-entry",
            "--worker-agent",
            "codex",
            "--worker-agent",
            "claude",
            "--worker-agent",
            "claude:entry-opus",
        ]),
        0,
    );
    let launches = f.launches();
    assert_eq!(
        launches
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["codex", "codex", "codex", "claude", "claude"]
    );
    assert_eq!(
        models(&launches),
        [
            "boss-entry",
            "gpt-6.1-sol-entry",
            "gpt-6.1-sol",
            "opus",
            "entry-opus"
        ]
    );
    f.clear_calls();
    status(
        &f.run(&[
            "test",
            "--worker-agent",
            "codex:one-model",
            "--workers",
            "2",
        ]),
        0,
    );
    assert_eq!(models(&f.launches()), ["fable", "one-model", "one-model"]);
}

fn entry_models_win_over_model_options() {
    let f = Fixture::new();
    status(
        &f.run(&[
            "test",
            "--boss-agent",
            "claude:boss-entry",
            "--worker-agent",
            "codex:entry",
            "--worker-agent",
            "claude",
            "--worker-model",
            "worker-option",
            "--model",
            "shared-option",
        ]),
        0,
    );
    assert_eq!(
        models(&f.launches()),
        ["boss-entry", "entry", "shared-option"]
    );
    f.clear_calls();
    status(
        &f.run(&[
            "test",
            "--worker-agent",
            "codex:entry",
            "--worker-agent",
            "claude",
            "--worker-model",
            "worker-option",
        ]),
        0,
    );
    assert_eq!(models(&f.launches()), ["fable", "entry", "worker-option"]);
}

fn zero_workers_and_explicit_models() {
    let f = Fixture::new();
    status(
        &f.run(&[
            "test",
            "--boss-agent",
            "codex",
            "--workers",
            "0",
            "--boss-model",
            "chosen-model",
        ]),
        0,
    );
    let launches = f.launches();
    assert_eq!(
        &launches[0][..4],
        ["codex", "--approve-for-me", "--model", "chosen-model"]
    );
    assert_eq!(launches.len(), 1);
    assert!(!f.calls().iter().any(|call| call[..2] == ["pane", "split"]));
}

fn model_and_initial_prompt_remain_single_arguments() {
    let f = Fixture::new();
    let model = "custom 'model'; touch SHOULD_NOT_EXIST; $(echo unsafe)";
    status(&f.run(&["test", "--workers", "1", "--model", model]), 0);
    for args in f.launches() {
        assert_eq!(models(std::slice::from_ref(&args)), [model]);
        assert_eq!(args.len(), 6);
    }
    assert!(!f.root.join("SHOULD_NOT_EXIST").exists());
}

fn invalid_arguments_create_no_panes() {
    let f = Fixture::new();
    let missing = f.root.join("missing");
    for args in [
        vec![],
        vec!["test", "--boss-agent", "other"],
        vec!["test", "--worker-agent", "other"],
        vec!["test", "--worker-agent", "other:model"],
        vec!["test", "--worker-agent", "codex:"],
        vec!["test", "--boss-agent", "claude:"],
        vec!["test", "--workers", "-1"],
        vec!["test", "--workers", "text"],
        vec!["test", "--workers", "10000"],
        vec!["test", "--cwd", missing.to_str().unwrap()],
        vec!["test", "--unknown"],
        vec!["test", "--boss-model"],
        vec!["test", "--cwd", "--dry-run"],
        vec!["test; echo unsafe"],
        vec!["test", "another-task"],
        vec![
            "test",
            "--workers",
            "3",
            "--worker-agent",
            "claude",
            "--worker-agent",
            "codex",
        ],
    ] {
        status(&f.run(&args), 2);
        assert!(f.calls().is_empty());
    }
}

fn dry_run_matches_real_commands_and_calls_no_programs() {
    let f = Fixture::new();
    let args = [
        "test",
        "--boss-agent",
        "codex",
        "--worker-agent",
        "claude",
        "--worker-agent",
        "codex",
    ];
    let mut dry_args = args.to_vec();
    dry_args.push("--dry-run");
    let dry = f.run(&dry_args);
    status(&dry, 0);
    assert!(f.calls().is_empty());
    let planned = Fixture::new();
    for line in stdout(&dry)
        .lines()
        .filter(|line| line.starts_with("  herdr "))
    {
        status(
            &output(planned.command("/bin/sh", &["-c", line.trim()]), ""),
            0,
        );
    }
    status(&f.run(&args), 0);
    let calls = f.calls();
    let mut mapping =
        std::collections::BTreeMap::from([("boss-pane".to_owned(), "<boss>".to_owned())]);
    for (index, call) in calls
        .iter()
        .filter(|call| call[..2] == ["agent", "rename"])
        .skip(1)
        .enumerate()
    {
        mapping.insert(call[2].clone(), format!("<w{}>", index + 1));
    }
    let actual: Vec<Vec<String>> = calls
        .iter()
        .filter(|call| call[..2] != ["pane", "focus"] && call[..2] != ["pane", "read"])
        .map(|call| {
            call.iter()
                .map(|arg| mapping.get(arg).unwrap_or(arg).clone())
                .collect()
        })
        .collect();
    assert_eq!(planned.calls(), actual);
}

fn symlink_launch_resolves_the_skill() {
    let f = Fixture::new();
    let link = f.bin.join("herd-boss");
    symlink(f.package.join("bin/herdr-shepherd"), &link).unwrap();
    status(&output(f.command(link, &["test", "--workers", "0"]), ""), 0);
    assert!(
        f.launches()[0].last().unwrap().contains(
            f.package
                .join("skills/herdr-shepherd/SKILL.md")
                .to_str()
                .unwrap()
        )
    );
}

fn missing_selected_binary_stops_before_tab_creation() {
    let f = Fixture::new();
    fs::remove_file(f.bin.join("codex")).unwrap();
    let result = f.run(&["test", "--boss-agent", "codex"]);
    status(&result, 2);
    assert!(stderr(&result).contains("codex is not on PATH"));
    assert!(f.calls().is_empty());
}

fn dry_run_needs_no_agent_or_herdr_binary() {
    let mut f = Fixture::new();
    f.env.insert(
        "PATH".into(),
        f.root.join("empty-path").to_str().unwrap().into(),
    );
    status(&f.run(&["test", "--boss-agent", "codex", "--dry-run"]), 0);
    assert!(f.calls().is_empty());
}

fn add_worker_starts_selected_agent_in_the_current_terminal() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    f.env.insert("HERD_AGENT_EXIT".into(), "7".into());
    status(
        &f.run(&[
            "test",
            "--add-worker",
            "--worker-agent",
            "codex",
            "--worker-model",
            "chosen-model",
        ]),
        7,
    );
    let args = f.agent_args();
    assert_eq!(Path::new(&args[0]).file_name().unwrap(), "codex");
    assert_eq!(
        &args[1..args.len() - 1],
        ["--approve-for-me", "--model", "chosen-model"]
    );
    assert!(args.last().unwrap().contains("You are test-w4."));
    assert!(args.last().unwrap().contains("Your boss is test-boss."));
    assert_eq!(
        f.mutations(),
        [
            vec!["agent", "rename", "current-pane", "test-w4"],
            vec!["pane", "rename", "current-pane", "test-w4"]
        ]
    );
}

fn add_worker_accepts_an_entry_model_from_a_shell_pane() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    status(
        &f.run(&[
            "test",
            "--add-worker",
            "--worker-agent",
            "codex:entry-model",
            "--worker-model",
            "option-model",
        ]),
        0,
    );
    let args = f.agent_args();
    assert_eq!(Path::new(&args[0]).file_name().unwrap(), "codex");
    assert_eq!(
        &args[1..args.len() - 1],
        ["--approve-for-me", "--model", "entry-model"]
    );
}

fn add_worker_starts_claude_by_default() {
    let mut f = Fixture::new();
    f.team(None, None, &[]);
    status(&f.run(&["test", "--add-worker"]), 0);
    let args = f.agent_args();
    assert_eq!(
        &args[..args.len() - 1],
        ["claude", "--name", "test-w1", "--model", "opus"]
    );
}

fn add_codex_worker_uses_the_worker_default_model() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    status(
        &f.run(&["test", "--add-worker", "--worker-agent", "codex"]),
        0,
    );
    let args = f.agent_args();
    assert_eq!(
        &args[1..args.len() - 1],
        ["--approve-for-me", "--model", "gpt-6.1-sol"]
    );
}

fn add_worker_preserves_each_running_agent_and_registration() {
    let mut f = Fixture::new();
    for kind in ["claude", "codex"] {
        f.team(Some(kind), None, &[1, 3]);
        for _ in 0..2 {
            let result = f.run(&["test", "--add-worker"]);
            status(&result, 0);
            assert!(stdout(&result).contains("test-w4"));
            assert!(!f.root.join("agent.json").exists());
        }
        let agents = f.agents();
        let agents = agents.as_array().unwrap();
        assert_eq!(
            agents
                .iter()
                .filter(|agent| agent["name"] == "test-w4")
                .count(),
            1
        );
        assert!(!agents.iter().any(|agent| agent["name"] == "test-w5"));
    }
}

fn add_worker_dry_run_does_not_register_or_start_an_agent() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    let result = f.run(&[
        "test",
        "--add-worker",
        "--worker-agent",
        "codex",
        "--dry-run",
    ]);
    status(&result, 0);
    for expected in ["test-w4", "--approve-for-me", "--model gpt-6.1-sol"] {
        assert!(stdout(&result).contains(expected));
    }
    assert!(f.mutations().is_empty());
    assert!(!f.root.join("agent.json").exists());
}

fn add_worker_rejects_unsafe_targets_before_changes() {
    let cases = [
        "outside-herdr",
        "stale-pane",
        "boss-pane",
        "other-team",
        "missing-boss",
        "duplicate-boss",
        "duplicate-worker",
        "other-agent",
        "change-agent",
        "change-model",
        "change-entry-model",
        "change-directory",
    ];
    for case in cases {
        let mut f = Fixture::new();
        let agent = match case {
            "duplicate-worker" | "change-agent" | "change-model" | "change-entry-model" => {
                Some("codex")
            }
            "other-agent" => Some("opencode"),
            "change-directory" => Some("claude"),
            _ => None,
        };
        let name = match case {
            "other-team" => Some("other-w1"),
            "duplicate-worker" => Some("test-w3"),
            _ => None,
        };
        f.team(agent, name, &[1, 3]);
        match case {
            "outside-herdr" => {
                f.env.remove("HERDR_PANE_ID");
            }
            "stale-pane" | "boss-pane" => {
                f.env.insert(
                    "HERDR_PANE_ID".into(),
                    if case == "stale-pane" {
                        "missing"
                    } else {
                        "boss-pane"
                    }
                    .into(),
                );
            }
            "missing-boss" | "duplicate-boss" => {
                let mut agents = f.agents().as_array().unwrap().clone();
                if case == "missing-boss" {
                    agents.remove(0);
                } else {
                    agents.push(agents[0].clone());
                }
                f.save_agents(&json!(agents));
            }
            _ => {}
        }
        let extra = match case {
            "change-agent" => vec!["--worker-agent", "claude"],
            "change-model" => vec!["--worker-model", "chosen-model"],
            "change-entry-model" => vec!["--worker-agent", "codex:chosen-model"],
            "change-directory" => vec!["--cwd", f.root.to_str().unwrap()],
            _ => vec![],
        };
        let mut args = vec!["test", "--add-worker"];
        args.extend(extra);
        status(&f.run(&args), 2);
        assert!(f.mutations().is_empty(), "{case}");
        assert!(!f.root.join("agent.json").exists(), "{case}");
    }
}

fn add_worker_reports_json_errors_even_when_herdr_exits_zero() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    f.env.insert("HERD_TEST_RENAME_ERROR".into(), "1".into());
    let result = f.run(&["test", "--add-worker"]);
    status(&result, 2);
    assert!(stderr(&result).contains("rename failed"));
    assert!(!f.root.join("agent.json").exists());
}

fn team_output_includes_workers_from_other_tabs() {
    let mut f = Fixture::new();
    f.team(Some("codex"), Some("test-w4"), &[1, 3]);
    for args in [vec!["test", "--team"], vec!["test", "--team", "--json"]] {
        let result = f.run(&args);
        status(&result, 0);
        if args.contains(&"--json") {
            let groups: Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(
                groups[0]["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|member| member["name"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["test-boss", "test-w1", "test-w3", "test-w4"]
            );
        } else {
            assert!(stdout(&result).contains("boss"));
            assert!(stdout(&result).contains("w4"));
        }
    }
    assert!(f.mutations().is_empty());
}

fn modes_reject_incompatible_options() {
    let f = Fixture::new();
    for args in [
        vec!["--json"],
        vec!["--team", "--add-worker"],
        vec!["--team", "--dry-run"],
        vec!["--team", "--worker-model", "model"],
        vec!["--add-worker", "--workers", "1"],
        vec![
            "--add-worker",
            "--worker-agent",
            "claude",
            "--worker-agent",
            "codex",
        ],
    ] {
        let mut full = vec!["test"];
        full.extend(args);
        status(&f.run(&full), 2);
        assert!(f.calls().is_empty());
    }
}

fn simultaneous_joins_allocate_distinct_worker_numbers() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    f.save_panes(&json!({"join-0": {"pane_id": "join-0"}, "join-1": {"pane_id": "join-1"}, "join-2": {"pane_id": "join-2"}}));
    let processes: Vec<_> = (0..3)
        .map(|index| {
            let mut command = f.command(
                f.package.join("bin/herdr-shepherd"),
                &["test", "--add-worker"],
            );
            command.env("HERDR_PANE_ID", format!("join-{index}"));
            thread::spawn(move || output(command, ""))
        })
        .collect();
    for process in processes {
        status(&process.join().unwrap(), 0);
    }
    let agents = f.agents();
    for name in ["test-w4", "test-w5", "test-w6"] {
        assert!(
            agents
                .as_array()
                .unwrap()
                .iter()
                .any(|agent| agent["name"] == name)
        );
    }
}

fn join_waits_for_the_compatible_flock_registration_lock() {
    let mut f = Fixture::new();
    f.team(Some("codex"), None, &[1, 3]);
    let lock = fs::File::create(f.lock_path()).unwrap();
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
    let command = f.command(
        f.package.join("bin/herdr-shepherd"),
        &["test", "--add-worker"],
    );
    let process = thread::spawn(move || output(command, ""));
    thread::sleep(Duration::from_millis(200));
    let waiting = !process.is_finished();
    let calls = f.calls();
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    let result = process.join().unwrap();
    assert!(waiting);
    assert!(calls.is_empty());
    status(&result, 0);
    assert!(stdout(&result).contains("test-w4"));
}

fn join_rejects_a_symlink_at_the_lock_path() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    let target = f.root.join("preserved-file");
    fs::write(&target, "keep this file").unwrap();
    symlink(&target, f.lock_path()).unwrap();
    assert!(!f.run(&["test", "--add-worker"]).status.success());
    assert!(f.calls().is_empty());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep this file");
}

fn relocated_package_runs_without_personal_configuration() {
    let mut f = Fixture::new();
    let original = f.package.clone();
    f.package = f.root.join("standalone checkout");
    copy_package(&f.package);
    fs::remove_dir_all(&original).unwrap();
    status(
        &f.run(&[
            "test",
            "--boss-agent",
            "codex",
            "--worker-agent",
            "claude",
            "--workers",
            "1",
            "--cwd",
            f.root.to_str().unwrap(),
        ]),
        0,
    );
    let launches = f.launches();
    assert_eq!(launches.len(), 2);
    for args in launches {
        assert!(
            args.last().unwrap().contains(
                f.package
                    .join("skills/herdr-shepherd/SKILL.md")
                    .to_str()
                    .unwrap()
            )
        );
        assert!(!args.last().unwrap().contains(original.to_str().unwrap()));
    }
}

fn legacy_command_and_version() {
    let f = Fixture::new();
    status(
        &output(
            f.command(f.package.join("bin/herd-boss"), &["test", "--workers", "0"]),
            "",
        ),
        0,
    );
    assert_eq!(f.launches().len(), 1);
    f.clear_calls();
    let result = f.run(&["--version"]);
    status(&result, 0);
    assert_eq!(stdout(&result).trim(), "herdr-shepherd 0.1.0");
    assert!(f.calls().is_empty());
}

fn add_worker_rejects_folder_mismatches_before_registration() {
    for running in [false, true] {
        let mut f = Fixture::new();
        f.team(if running { Some("codex") } else { None }, None, &[1, 3]);
        let other = f.root.join("other");
        fs::create_dir(&other).unwrap();
        let mut args = vec!["test", "--add-worker"];
        if running {
            let mut panes = read_json(f.root.join("panes.json"));
            panes["current-pane"]["cwd"] = json!(other);
            f.save_panes(&panes);
        } else {
            args.extend(["--cwd", other.to_str().unwrap()]);
        }
        let result = f.run(&args);
        status(&result, 2);
        assert!(stderr(&result).contains("boss folder"));
        assert!(f.mutations().is_empty());
        assert!(!f.root.join("agent.json").exists());
    }
}

fn server_errors_stop_launch_even_when_herdr_exits_zero() {
    let mut f = Fixture::new();
    for operation in [
        "tab create",
        "pane rename",
        "agent rename",
        "pane send-text",
    ] {
        f.clear_calls();
        f.env.insert("HERD_TEST_ERROR".into(), operation.into());
        let result = f.run(&["test", "--workers", "0"]);
        assert!(!result.status.success());
        assert!(stderr(&result).contains("requested operation failed"));
        assert!(
            !f.calls()
                .iter()
                .any(|call| call[..2] == ["pane", "send-keys"])
        );
    }
}

fn empty_pane_ids_stop_launch_before_targeting_another_pane() {
    let mut f = Fixture::new();
    for operation in ["tab create", "pane split"] {
        f.clear_calls();
        f.env
            .insert("HERD_TEST_INVALID_PANE".into(), operation.into());
        let result = f.run(&["test", "--workers", "1"]);
        assert!(!result.status.success());
        assert!(stderr(&result).contains("pane_id"));
        assert!(
            !f.calls()
                .iter()
                .any(|call| call.iter().any(String::is_empty))
        );
    }
}

fn team_list_excludes_names_without_a_team_role() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    let mut agents = f.agents().as_array().unwrap().clone();
    let mut other = agents[0].clone();
    other["name"] = json!("test-helper");
    other["pane_id"] = json!("unrelated-pane");
    agents.push(other);
    f.save_agents(&json!(agents));
    let result = f.run(&["test", "--team", "--json"]);
    status(&result, 0);
    let groups: Value = serde_json::from_slice(&result.stdout).unwrap();
    let members = groups[0]["members"].as_array().unwrap();
    assert_eq!(
        members
            .iter()
            .map(|member| member["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["test-boss", "test-w1", "test-w3"]
    );
    assert!(
        members
            .iter()
            .all(|member| member["cwd"] == f.root.to_str().unwrap())
    );
}

fn team_list_reports_server_errors() {
    let mut f = Fixture::new();
    f.env.insert("HERD_TEST_ERROR".into(), "agent list".into());
    let result = f.run(&["test", "--team", "--json"]);
    assert!(!result.status.success());
    assert!(stderr(&result).contains("requested operation failed"));
    assert!(result.stdout.is_empty());
}

fn successful_mutations_can_return_no_output() {
    let mut f = Fixture::new();
    for operation in ["pane send-text", "pane send-keys"] {
        f.clear_calls();
        f.env.insert("HERD_TEST_EMPTY".into(), operation.into());
        status(&f.run(&["test", "--workers", "0"]), 0);
        assert!(
            f.calls()
                .iter()
                .any(|call| call[..2] == ["pane", "send-keys"])
        );
    }
}

fn viewer_uses_agent_type_for_names_without_a_team_role() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    let mut agents = f.agents().as_array().unwrap().clone();
    let mut other = agents[0].clone();
    other["name"] = json!("api-review");
    other["pane_id"] = json!("unrelated-pane");
    agents.push(other);
    f.save_agents(&json!(agents));
    let result = output(f.command(f.package.join("bin/herd-view"), &["--json"]), "");
    status(&result, 0);
    let groups: Value = serde_json::from_slice(&result.stdout).unwrap();
    let member = groups
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["members"].as_array().unwrap())
        .find(|member| member["name"] == "api-review")
        .unwrap();
    assert_eq!(member["role"], "codex");
}

fn omitted_task_uses_the_boss_in_the_current_folder() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    let result = f.run(&["--team", "--json"]);
    status(&result, 0);
    let groups: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(groups[0]["subject"], "test");
    let result = f.run(&["--add-worker", "--worker-agent", "codex"]);
    status(&result, 0);
    assert!(stdout(&result).contains("test-w4"));
    f.team(None, None, &[1, 3]);
    f.clear_calls();
    let mut agents = f.agents().as_array().unwrap().clone();
    let mut other = agents[0].clone();
    other["name"] = json!("other-boss");
    other["pane_id"] = json!("other-boss-pane");
    agents.push(other);
    f.save_agents(&json!(agents));
    for args in [vec!["--team"], vec!["--add-worker"]] {
        let result = f.run(&args);
        status(&result, 2);
        assert!(stderr(&result).contains("Several teams run in this folder: other, test."));
        assert!(f.mutations().is_empty());
    }
    agents[0]["cwd"] = json!(f.home.to_str().unwrap());
    agents.pop();
    f.save_agents(&json!(agents));
    for args in [vec!["--team"], vec!["--add-worker"]] {
        let result = f.run(&args);
        status(&result, 2);
        assert!(stderr(&result).contains("No team runs in this folder."));
        assert!(f.mutations().is_empty());
    }
    status(&f.run(&[]), 2);
}

fn send_stores_the_message_and_types_a_short_pointer() {
    let mut f = Fixture::new();
    f.team(None, None, &[1, 3]);
    let long = format!(
        "{} $HOME 'quoted' \"double\"\nsecond line",
        "x".repeat(6000)
    );
    let result = f.input(&["--send", "w1"], &long);
    status(&result, 0);
    let calls = f.calls();
    let send = calls
        .iter()
        .find(|call| call[..2] == ["agent", "send"])
        .unwrap();
    assert_eq!(send[2], "test-w1");
    assert!(send[3].len() < 120, "{}", send[3]);
    assert!(!send[3].contains('$'));
    assert!(calls.contains(&vec![
        "pane".to_owned(),
        "send-keys".into(),
        "pane-1".into(),
        "Enter".into()
    ]));
    let id = send[3].rsplit(' ').next().unwrap().to_owned();
    assert!(send[3].contains(&format!("herdr-shepherd test --read {id}")));
    f.env.insert("HERDR_PANE_ID".into(), "pane-1".into());
    let result = f.run(&["--read"]);
    status(&result, 0);
    assert!(stdout(&result).contains(&format!("{id}  From: user  xxx")));
    let result = f.run(&["test", "--read", &id]);
    status(&result, 0);
    assert!(stdout(&result).starts_with("From: user\nTo: test-w1\n\nxxx"));
    assert!(stdout(&result).contains(&long));
    let result = f.run(&["--read"]);
    status(&result, 0);
    assert!(stdout(&result).contains("No unread messages for test-w1."));
    status(&f.run(&["--read", &id]), 0);
    f.clear_calls();
    status(&f.run(&["--send", "boss", "--message", "done"]), 0);
    let calls = f.calls();
    let send = calls
        .iter()
        .find(|call| call[..2] == ["agent", "send"])
        .unwrap();
    assert_eq!(send[2], "test-boss");
    assert!(send[3].contains("from test-w1."));
    assert!(
        calls
            .iter()
            .any(|call| call.len() > 3 && call[..3] == ["pane", "send-keys", "boss-pane"])
    );
    f.clear_calls();
    status(
        &f.run(&["--send", "w2", "--message", "dry", "--dry-run"]),
        0,
    );
    assert!(f.mutations().is_empty());
    for args in [
        vec!["--send", "w9", "--message", "x"],
        vec!["--send", "--message", "x"],
        vec!["--read", "missing-id"],
        vec!["--read", "bad/id"],
        vec!["--message", "x"],
        vec!["--send", "w1", "--message", "x", "--workers", "2"],
        vec!["--send", "w1", "--read"],
    ] {
        status(&f.run(&args), 2);
    }
    status(&f.input(&["--send", "w1"], " \n"), 2);
    assert!(f.mutations().is_empty());
    let mailbox = f.home.join(".local/state/herdr-shepherd/mail/test");
    let stale = mailbox.join("m1-1.md");
    fs::write(&stale, "From: user\nTo: test-w1\n\nold\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60))
        .unwrap();
    let result = f.run(&["--read"]);
    status(&result, 0);
    assert!(!stdout(&result).contains("m1-1"));
    assert!(!stale.exists());
    assert!(mailbox.join(format!("{id}.read.md")).exists());
    status(&f.run(&["test", "--workers", "0"]), 0);
    assert!(!mailbox.exists());
}

fn team_list_explains_unreachable_server() {
    let mut f = Fixture::new();
    f.env.insert("HERD_TEST_OFFLINE".into(), "1".into());
    let result = f.run(&["test", "--team"]);
    assert!(!result.status.success());
    assert!(stderr(&result).contains("Check that the herdr server is running"));
    assert!(stderr(&result).contains("socket unavailable"));
    assert!(result.stdout.is_empty());
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
    if matches!(name, "herdr" | "claude" | "codex") {
        std::process::exit(fixture_process(name, &args[1..]).unwrap_or_else(|error| {
            eprintln!("{error}");
            1
        }));
    }
    if args.get(1).map(String::as_str) == Some("--pty-supervisor") {
        std::process::exit(terminal_supervisor(&args[2..]).unwrap_or_else(|error| {
            eprintln!("{error}");
            1
        }));
    }
    // Reuse this test executable as the fake commands and the terminal supervisor.
    macro_rules! cases { ($($name:ident),* $(,)?) => { vec![$((stringify!($name), $name as fn())),*] }; }
    let cases = cases![
        install_is_repeatable_and_registers_both_agents,
        existing_command_stops_install_before_any_new_links,
        existing_skill_directory_is_preserved,
        stale_link_is_preserved,
        missing_agent_stops_install_before_writes,
        unsupported_herdr_stops_install_before_writes,
        check_mode_does_not_install_missing_links,
        relocated_checkout_installs_and_runs_without_original_paths,
        interactive_defaults_preview_without_external_commands,
        interactive_choices_use_the_existing_launch_path,
        interactive_worker_list_uses_each_entry,
        interactive_expands_home_and_allows_zero_workers,
        interactive_help_does_not_prompt,
        interactive_cancel_eof_and_conflicting_options_create_no_panes,
        no_arguments_open_ratatui_and_cancel_restores_the_terminal,
        ratatui_preview_uses_choices_and_restores_the_terminal,
        ratatui_restores_the_terminal_before_a_launch_error,
        small_terminal_can_cancel_without_creating_a_team,
        default_launch_preserves_claude_models_and_names,
        each_boss_type_supports_mixed_workers,
        single_worker_type_applies_to_the_worker_count,
        codex_model_options_override_role_defaults,
        agent_entries_select_models_in_a_mixed_list,
        entry_models_win_over_model_options,
        zero_workers_and_explicit_models,
        model_and_initial_prompt_remain_single_arguments,
        invalid_arguments_create_no_panes,
        dry_run_matches_real_commands_and_calls_no_programs,
        symlink_launch_resolves_the_skill,
        missing_selected_binary_stops_before_tab_creation,
        dry_run_needs_no_agent_or_herdr_binary,
        add_worker_starts_selected_agent_in_the_current_terminal,
        add_worker_accepts_an_entry_model_from_a_shell_pane,
        add_worker_starts_claude_by_default,
        add_codex_worker_uses_the_worker_default_model,
        add_worker_preserves_each_running_agent_and_registration,
        add_worker_dry_run_does_not_register_or_start_an_agent,
        add_worker_rejects_unsafe_targets_before_changes,
        add_worker_reports_json_errors_even_when_herdr_exits_zero,
        team_output_includes_workers_from_other_tabs,
        omitted_task_uses_the_boss_in_the_current_folder,
        send_stores_the_message_and_types_a_short_pointer,
        modes_reject_incompatible_options,
        simultaneous_joins_allocate_distinct_worker_numbers,
        join_waits_for_the_compatible_flock_registration_lock,
        join_rejects_a_symlink_at_the_lock_path,
        relocated_package_runs_without_personal_configuration,
        legacy_command_and_version,
        add_worker_rejects_folder_mismatches_before_registration,
        server_errors_stop_launch_even_when_herdr_exits_zero,
        empty_pane_ids_stop_launch_before_targeting_another_pane,
        team_list_excludes_names_without_a_team_role,
        team_list_reports_server_errors,
        successful_mutations_can_return_no_output,
        viewer_uses_agent_type_for_names_without_a_team_role,
        team_list_explains_unreachable_server,
    ];
    let filter = args
        .iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("");
    let mut failures = 0;
    let mut count = 0;
    for (name, test) in cases.into_iter().filter(|(name, _)| name.contains(filter)) {
        count += 1;
        let passed = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if passed { "ok" } else { "FAILED" });
        if !passed {
            failures += 1;
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failures} failed",
        if failures == 0 { "ok" } else { "FAILED" },
        count - failures
    );
    if failures > 0 {
        std::process::exit(1);
    }
}
