//! `run --resume [SESSION] PROMPT`, the documented argument shape.
//!
//! `--resume` takes an optional value (omitted, it resumes `last`) and `run`
//! requires the prompt. Clap parses an optional value greedily, so in
//! `rkat run --resume "keep going"` it would take the prompt as the session
//! and then report the prompt missing. The documented reading is that a bare
//! `--resume` followed by the run's only positional resumes `last` with that
//! positional as the prompt. [`imply_resume_last`] makes that reading explicit
//! by inserting `last` before clap parses, using the `run` command's own
//! argument definitions to tell flag values from positionals.

use std::ffi::OsString;

/// Insert `last` after the first bare `--resume` among the `run` arguments
/// (`args[run_index + 1..]`) when the word clap would take as its value is
/// the run's only positional: that word is the prompt.
///
/// A word that names a session (`names_session`) stays the session, and
/// clap then reports the missing prompt. `--resume=VALUE`, a `--resume`
/// followed by an option or by nothing, and a run with another positional
/// are left unchanged.
pub(crate) fn imply_resume_last(
    args: &mut Vec<OsString>,
    run_index: usize,
    run: &clap::Command,
    names_session: impl Fn(&str) -> bool,
) {
    let mut resume_value = None;
    let mut positionals = 0_usize;
    let mut index = run_index + 1;
    while let Some(token) = args.get(index) {
        index += 1;
        let Some(token) = token.to_str() else {
            positionals += 1;
            continue;
        };
        if token == "--" {
            positionals += args.len() - index;
            break;
        }
        if let Some(long) = token.strip_prefix("--") {
            if long.contains('=') {
                continue;
            }
            let arg = long_arg(run, long);
            if arg.is_some_and(|arg| arg.get_id() == "resume") {
                if resume_value.is_none() && value_count(arg, &args[index..]) == 1 {
                    resume_value = Some(index);
                }
                index += value_count(arg, &args[index..]);
                continue;
            }
            index += value_count(arg, &args[index..]);
            continue;
        }
        if let Some(shorts) = token.strip_prefix('-')
            && !shorts.is_empty()
        {
            index += short_cluster_value_count(run, shorts, &args[index..]);
            continue;
        }
        positionals += 1;
    }
    if positionals == 0
        && let Some(value_index) = resume_value
        && args[value_index]
            .to_str()
            .is_some_and(|value| !names_session(value))
    {
        args.insert(value_index, OsString::from("last"));
    }
}

/// The `run` argument spelled `--long` (or one of its aliases).
fn long_arg<'a>(run: &'a clap::Command, long: &str) -> Option<&'a clap::Arg> {
    run.get_arguments().find(|arg| {
        arg.get_long() == Some(long)
            || arg
                .get_all_aliases()
                .is_some_and(|aliases| aliases.contains(&long))
    })
}

/// How many following words a short-option cluster (`-vm`, `-mVALUE`)
/// consumes: the value of its last option when that option takes one and
/// its value is not attached.
fn short_cluster_value_count(run: &clap::Command, shorts: &str, rest: &[OsString]) -> usize {
    for (offset, short) in shorts.char_indices() {
        let Some(arg) = run.get_arguments().find(|arg| {
            arg.get_short() == Some(short)
                || arg
                    .get_all_short_aliases()
                    .is_some_and(|aliases| aliases.contains(&short))
        }) else {
            return 0;
        };
        if arg.get_action().takes_values() {
            let attached = offset + short.len_utf8() < shorts.len();
            return if attached {
                0
            } else {
                value_count(Some(arg), rest)
            };
        }
    }
    0
}

/// How many of the following words clap takes as values of `arg` given
/// without an attached value: up to its maximum, stopping at the next
/// option unless the argument accepts hyphen-led values.
fn value_count(arg: Option<&clap::Arg>, rest: &[OsString]) -> usize {
    let Some(arg) = arg else {
        return 0;
    };
    if !arg.get_action().takes_values() {
        return 0;
    }
    let max = arg.get_num_args().map_or(1, |range| range.max_values());
    let hyphen_values = arg.is_allow_hyphen_values_set();
    let mut taken = 0;
    while taken < max {
        let Some(word) = rest.get(taken) else {
            break;
        };
        let option_like = word
            .to_str()
            .is_some_and(|word| word.len() > 1 && word.starts_with('-'));
        if option_like && !hyphen_values {
            break;
        }
        taken += 1;
    }
    taken
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use clap::Parser;

    /// The parsed `(resume, prompt, model)` of an `rkat` invocation, through
    /// the same normalization the binary applies.
    fn parse_run(words: &[&str]) -> Result<(Option<String>, String, Option<String>), String> {
        let args = crate::normalize_cli_args(words.iter().map(std::ffi::OsString::from));
        let cli = crate::Cli::try_parse_from(args).map_err(|error| error.to_string())?;
        match cli.command {
            Some(crate::Commands::Run {
                resume,
                prompt,
                model,
                ..
            }) => Ok((resume, prompt, model)),
            _ => panic!("{words:?} must parse as run"),
        }
    }

    fn resumed(session: &str, prompt: &str) -> (Option<String>, String, Option<String>) {
        (Some(session.to_string()), prompt.to_string(), None)
    }

    /// The documented forms: `--resume` without a value resumes `last` and
    /// the lone positional is the prompt, with options in any position.
    #[test]
    fn run_resume_takes_a_lone_positional_as_the_prompt() {
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "keep going"]),
            Ok(resumed("last", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "keep going", "-m", "gpt-5.5"]),
            Ok((
                Some("last".to_string()),
                "keep going".to_string(),
                Some("gpt-5.5".to_string())
            ))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "-m", "gpt-5.5", "--resume", "keep going"]),
            Ok((
                Some("last".to_string()),
                "keep going".to_string(),
                Some("gpt-5.5".to_string())
            ))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--model=gpt-5.5", "--resume", "keep going"]),
            Ok((
                Some("last".to_string()),
                "keep going".to_string(),
                Some("gpt-5.5".to_string())
            ))
        );
        assert_eq!(
            parse_run(&["rkat", "--resume", "keep going", "-m", "gpt-5.5"]),
            Ok((
                Some("last".to_string()),
                "keep going".to_string(),
                Some("gpt-5.5".to_string())
            ))
        );
        assert_eq!(
            parse_run(&["rkat", "--realm", "team", "run", "--resume", "keep going"]),
            Ok(resumed("last", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "keep going", "--stdin", "blob"]),
            Ok(resumed("last", "keep going"))
        );
    }

    /// A named session stays the session: an explicit value, a value next to
    /// a prompt, and a bare `--resume` before an option or after the prompt.
    #[test]
    fn run_resume_keeps_an_explicit_session() {
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "last", "keep going"]),
            Ok(resumed("last", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "~2", "pick this thread back up"]),
            Ok(resumed("~2", "pick this thread back up"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "019c8b99", "keep going"]),
            Ok(resumed("019c8b99", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume=019c8b99", "keep going"]),
            Ok(resumed("019c8b99", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "keep going", "--resume", "019c8b99"]),
            Ok(resumed("019c8b99", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "keep going", "--resume"]),
            Ok(resumed("last", "keep going"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "--stdin", "lines", "watch"]),
            Ok(resumed("last", "watch"))
        );
        assert_eq!(
            parse_run(&["rkat", "run", "--resume", "--", "keep going"]),
            Ok(resumed("last", "keep going"))
        );
    }

    /// A lone word that names a session is not taken as the prompt: the
    /// prompt is still required.
    #[test]
    fn run_resume_with_only_a_session_still_requires_the_prompt() {
        for session in ["019c8b99", "last", "~2"] {
            for words in [
                ["rkat", "run", "--resume", session].as_slice(),
                ["rkat", "--resume", session].as_slice(),
            ] {
                let error = parse_run(words).expect_err("the prompt is required");
                assert!(error.contains("<PROMPT>"), "{words:?}: {error}");
            }
        }
    }
}
