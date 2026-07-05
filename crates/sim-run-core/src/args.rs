use std::{ffi::OsString, path::PathBuf};

use crate::{CliBoot, CliError, LibSourceSpec};

/// Top-level command selected by the bootloader parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliCommand {
    /// Print the bootloader help text and exit.
    Help,
    /// Print the binary version and exit.
    Version,
    /// Load libraries and hand off to a loaded entrypoint.
    Boot(CliBoot),
}

/// Parses the minimal bootloader flags.
///
/// The first argument is treated as the program name and skipped, matching
/// `std::env::args_os`. An empty argument list selects [`CliCommand::Help`].
///
/// # Examples
///
/// ```
/// use sim_run_core::{parse_args, CliCommand};
///
/// assert_eq!(parse_args(["sim", "--version"]).unwrap(), CliCommand::Version);
///
/// let CliCommand::Boot(boot) = parse_args(["sim", "--codec", "json"]).unwrap() else {
///     panic!("expected a boot command");
/// };
/// assert_eq!(boot.codec.as_deref(), Some("json"));
/// ```
pub fn parse_args<I, S>(args: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    if !args.is_empty() {
        args.remove(0);
    }
    if args.is_empty() {
        return Ok(CliCommand::Help);
    }

    let mut boot = CliBoot::default();
    let mut cursor = 0;
    while cursor < args.len() {
        let arg = arg_string(&args[cursor]);
        match arg.as_str() {
            "--" => {
                boot.payload.args.extend(args.drain(cursor + 1..));
                break;
            }
            "--help" | "-h" | "help" => return Ok(CliCommand::Help),
            "--version" | "-V" | "version" => return Ok(CliCommand::Version),
            "--list" => {
                boot.list = true;
                cursor += 1;
            }
            "--codec" => {
                boot.codec = set_once(boot.codec, "--codec", take_value(&args, &mut cursor)?)?;
            }
            "--load" => {
                let source = take_value(&args, &mut cursor)?;
                boot.loads.push(source.parse::<LibSourceSpec>()?);
            }
            "--native-audio-provider" => {
                let source = take_value(&args, &mut cursor)?;
                boot.native_audio_provider = set_once(
                    boot.native_audio_provider,
                    "--native-audio-provider",
                    Box::new(source.parse::<LibSourceSpec>()?),
                )?;
            }
            "--inspect" => {
                boot.inspect =
                    set_once(boot.inspect, "--inspect", take_value(&args, &mut cursor)?)?;
            }
            "--eval" => {
                boot.payload.eval =
                    set_once(boot.payload.eval, "--eval", take_value(&args, &mut cursor)?)?;
            }
            "--script" => {
                let script = PathBuf::from(take_value(&args, &mut cursor)?);
                boot.payload.script = set_once(boot.payload.script, "--script", script)?;
            }
            "--stdin" => {
                boot.payload.stdin = set_once(
                    boot.payload.stdin,
                    "--stdin",
                    take_value(&args, &mut cursor)?,
                )?;
            }
            _ if arg.starts_with("--codec=") => {
                boot.codec = set_once(boot.codec, "--codec", inline_value(&arg, "--codec=")?)?;
                cursor += 1;
            }
            _ if arg.starts_with("--load=") => {
                boot.loads
                    .push(inline_value(&arg, "--load=")?.parse::<LibSourceSpec>()?);
                cursor += 1;
            }
            _ if arg.starts_with("--native-audio-provider=") => {
                boot.native_audio_provider = set_once(
                    boot.native_audio_provider,
                    "--native-audio-provider",
                    Box::new(
                        inline_value(&arg, "--native-audio-provider=")?.parse::<LibSourceSpec>()?,
                    ),
                )?;
                cursor += 1;
            }
            _ if arg.starts_with("--inspect=") => {
                boot.inspect =
                    set_once(boot.inspect, "--inspect", inline_value(&arg, "--inspect=")?)?;
                cursor += 1;
            }
            _ if arg.starts_with("--eval=") => {
                boot.payload.eval =
                    set_once(boot.payload.eval, "--eval", inline_value(&arg, "--eval=")?)?;
                cursor += 1;
            }
            _ if arg.starts_with("--script=") => {
                let script = PathBuf::from(inline_value(&arg, "--script=")?);
                boot.payload.script = set_once(boot.payload.script, "--script", script)?;
                cursor += 1;
            }
            _ if arg.starts_with("--stdin=") => {
                boot.payload.stdin = set_once(
                    boot.payload.stdin,
                    "--stdin",
                    inline_value(&arg, "--stdin=")?,
                )?;
                cursor += 1;
            }
            _ if !arg.starts_with('-') => {
                boot.payload.args.extend(args.drain(cursor..));
                break;
            }
            _ if arg.starts_with('-') => return Err(CliError::unsupported(&arg)),
            _ => return Err(CliError::unsupported(&arg)),
        }
    }

    Ok(CliCommand::Boot(boot))
}

fn take_value(args: &[OsString], cursor: &mut usize) -> Result<String, CliError> {
    let flag = arg_string(&args[*cursor]);
    let Some(value) = args.get(*cursor + 1) else {
        return Err(CliError::missing_value(&flag));
    };
    *cursor += 2;
    Ok(arg_string(value))
}

fn inline_value(arg: &str, prefix: &str) -> Result<String, CliError> {
    let value = &arg[prefix.len()..];
    if value.is_empty() {
        Err(CliError::missing_value(prefix.trim_end_matches('=')))
    } else {
        Ok(value.to_owned())
    }
}

fn set_once<T>(slot: Option<T>, flag: &str, value: T) -> Result<Option<T>, CliError> {
    if slot.is_some() {
        Err(CliError::duplicate(flag))
    } else {
        Ok(Some(value))
    }
}

fn arg_string(arg: &OsString) -> String {
    arg.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Payload, source::LibSourceSpec};

    #[test]
    fn parses_boot_flags_and_repeated_loads() {
        let parsed = parse_args([
            "sim",
            "--codec",
            "json",
            "--load",
            "symbol:codec/json",
            "--load=path:./lib.wasm",
            "--native-audio-provider=symbol:audio/provider/jack",
            "--list",
            "--inspect",
            "codec/json",
            "--eval",
            "(+ 1 2)",
            "--script=demo.sim",
            "--stdin",
            "input",
        ])
        .unwrap();

        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(boot.codec, Some("json".to_owned()));
        assert_eq!(
            boot.loads,
            vec![
                LibSourceSpec::Symbol("codec/json".to_owned()),
                LibSourceSpec::Path(PathBuf::from("./lib.wasm")),
            ]
        );
        assert_eq!(
            boot.native_audio_provider.as_deref(),
            Some(&LibSourceSpec::Symbol("audio/provider/jack".to_owned()))
        );
        assert!(boot.list);
        assert_eq!(boot.inspect, Some("codec/json".to_owned()));
        assert_eq!(
            boot.payload,
            Payload {
                args: Vec::new(),
                eval: Some("(+ 1 2)".to_owned()),
                script: Some(PathBuf::from("demo.sim")),
                stdin: Some("input".to_owned()),
            }
        );
    }

    #[test]
    fn payload_after_double_dash_is_preserved_and_not_rejected() {
        let parsed = parse_args(["sim", "--codec=lisp", "--", "run", "--flag", "value"]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(
            boot.payload.args,
            vec![
                OsString::from("run"),
                OsString::from("--flag"),
                OsString::from("value"),
            ]
        );
        assert_eq!(boot.envelope().verb, Some("run".to_owned()));
    }

    #[test]
    fn parses_native_audio_provider_opt_in() {
        let parsed =
            parse_args(["sim", "--native-audio-provider=path:./jack-provider.so"]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(
            boot.native_audio_provider.as_deref(),
            Some(&LibSourceSpec::Path(PathBuf::from("./jack-provider.so")))
        );
    }

    #[test]
    fn first_payload_token_starts_loaded_lib_handoff() {
        let parsed = parse_args(["sim", "--load=host:demo", "run", "--flag"]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(
            boot.payload.args,
            vec![OsString::from("run"), OsString::from("--flag")]
        );
        assert_eq!(boot.envelope().verb, Some("run".to_owned()));
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_flags() {
        assert_eq!(
            parse_args(["sim", "--codec"]).unwrap_err().to_string(),
            "--codec requires a value"
        );
        assert_eq!(
            parse_args(["sim", "--codec=lisp", "--codec=json"])
                .unwrap_err()
                .to_string(),
            "--codec was provided more than once"
        );
        assert_eq!(
            parse_args(["sim", "--unknown"]).unwrap_err().to_string(),
            "unsupported argument: --unknown"
        );
    }
}
