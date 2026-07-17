use std::{ffi::OsString, path::PathBuf};

use crate::{
    CliBoot, CliError, ConfigReportKind, ConfigReportRequest, LibSourceSpec,
    source::{parse_source_os, symbol_from_text},
};

/// Top-level command selected by the bootloader parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliCommand {
    /// Print the bootloader help text and exit.
    Help,
    /// Print the binary version and exit.
    Version,
    /// Load libraries and hand off to a loaded entrypoint.
    Boot(Box<CliBoot>),
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
    let mut seen = ConfigFlagsSeen::default();
    let mut cursor = 0;
    while cursor < args.len() {
        let arg = match arg_text(&args[cursor], "argument flag") {
            Ok(arg) => arg.to_owned(),
            Err(_) if is_positional_os_arg(&args[cursor]) => {
                boot.payload.args.extend(args.drain(cursor..));
                break;
            }
            Err(err) => return Err(err),
        };
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
                boot.codec = set_once(boot.codec, "--codec", take_text_value(&args, &mut cursor)?)?;
            }
            "--load" => {
                let source = take_os_value(&args, &mut cursor)?;
                boot.loads.push(parse_source_os(source)?);
            }
            "--native-audio-provider" => {
                let source = take_os_value(&args, &mut cursor)?;
                boot.native_audio_provider = set_once(
                    boot.native_audio_provider,
                    "--native-audio-provider",
                    Box::new(parse_source_os(source)?),
                )?;
            }
            "--config-home" => {
                reject_seen(&mut seen.home, "--config-home")?;
                boot.config.roots.home = Some(PathBuf::from(take_os_value(&args, &mut cursor)?));
            }
            "--config-work" => {
                reject_seen(&mut seen.work, "--config-work")?;
                boot.config.roots.work = PathBuf::from(take_os_value(&args, &mut cursor)?);
            }
            "--config-file" => {
                boot.config.single_file = set_once(
                    boot.config.single_file,
                    "--config-file",
                    PathBuf::from(take_os_value(&args, &mut cursor)?),
                )?;
            }
            "--config-site" => {
                let site = symbol_from_text(&take_text_value(&args, &mut cursor)?);
                boot.config.site_sources.push(site);
            }
            "--no-config-files" => {
                reject_seen(&mut seen.no_files, "--no-config-files")?;
                boot.config.read_files = false;
                cursor += 1;
            }
            "--inspect" => {
                boot.inspect = set_once(
                    boot.inspect,
                    "--inspect",
                    take_text_value(&args, &mut cursor)?,
                )?;
            }
            "--eval" => {
                boot.payload.eval = set_once(
                    boot.payload.eval,
                    "--eval",
                    take_text_value(&args, &mut cursor)?,
                )?;
            }
            "--script" => {
                let script = PathBuf::from(take_os_value(&args, &mut cursor)?);
                boot.payload.script = set_once(boot.payload.script, "--script", script)?;
            }
            "--stdin" => {
                boot.payload.stdin = set_once(
                    boot.payload.stdin,
                    "--stdin",
                    take_text_value(&args, &mut cursor)?,
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
            _ if arg.starts_with("--config-home=") => {
                reject_seen(&mut seen.home, "--config-home")?;
                boot.config.roots.home = Some(PathBuf::from(inline_value(&arg, "--config-home=")?));
                cursor += 1;
            }
            _ if arg.starts_with("--config-work=") => {
                reject_seen(&mut seen.work, "--config-work")?;
                boot.config.roots.work = PathBuf::from(inline_value(&arg, "--config-work=")?);
                cursor += 1;
            }
            _ if arg.starts_with("--config-file=") => {
                boot.config.single_file = set_once(
                    boot.config.single_file,
                    "--config-file",
                    PathBuf::from(inline_value(&arg, "--config-file=")?),
                )?;
                cursor += 1;
            }
            _ if arg.starts_with("--config-site=") => {
                let site = symbol_from_text(&inline_value(&arg, "--config-site=")?);
                boot.config.site_sources.push(site);
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
                if arg == "config" {
                    boot.config_report = Some(parse_config_report(&args[cursor + 1..])?);
                    break;
                }
                boot.payload.args.extend(args.drain(cursor..));
                break;
            }
            _ if arg.starts_with('-') => return Err(CliError::unsupported(&arg)),
            _ => return Err(CliError::unsupported(&arg)),
        }
    }

    Ok(CliCommand::Boot(Box::new(boot)))
}

fn take_text_value(args: &[OsString], cursor: &mut usize) -> Result<String, CliError> {
    let flag = arg_text(&args[*cursor], "argument flag")?.to_owned();
    let Some(value) = args.get(*cursor + 1) else {
        return Err(CliError::missing_value(&flag));
    };
    *cursor += 2;
    Ok(arg_text(value, &flag)?.to_owned())
}

fn take_os_value(args: &[OsString], cursor: &mut usize) -> Result<OsString, CliError> {
    let flag = arg_text(&args[*cursor], "argument flag")?.to_owned();
    let Some(value) = args.get(*cursor + 1) else {
        return Err(CliError::missing_value(&flag));
    };
    *cursor += 2;
    Ok(value.clone())
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

#[derive(Default)]
struct ConfigFlagsSeen {
    home: bool,
    work: bool,
    no_files: bool,
}

fn reject_seen(seen: &mut bool, flag: &str) -> Result<(), CliError> {
    if *seen {
        Err(CliError::duplicate(flag))
    } else {
        *seen = true;
        Ok(())
    }
}

fn parse_config_report(args: &[OsString]) -> Result<ConfigReportRequest, CliError> {
    let mut json = false;
    let mut positionals = Vec::new();
    for arg in args {
        let arg = arg_text(arg, "config command argument")?.to_owned();
        match arg.as_str() {
            "--json" => {
                if json {
                    return Err(CliError::duplicate("--json"));
                }
                json = true;
            }
            "--" => return Err(CliError::unsupported("--")),
            _ if arg.starts_with('-') => return Err(CliError::unsupported(&arg)),
            _ => positionals.push(arg),
        }
    }
    let Some(command) = positionals.first().map(String::as_str) else {
        return Err(CliError::new(
            "config requires one of: status, effective, sources",
        ));
    };
    let kind = match command {
        "status" if positionals.len() == 1 => ConfigReportKind::Status,
        "sources" if positionals.len() == 1 => ConfigReportKind::Sources,
        "effective" if positionals.len() == 2 => ConfigReportKind::Effective {
            lib: symbol_from_text(&positionals[1]),
        },
        "effective" => {
            return Err(CliError::new(
                "config effective requires exactly one library id",
            ));
        }
        _ => return Err(CliError::unsupported(&format!("config {command}"))),
    };
    Ok(ConfigReportRequest { kind, json })
}

fn arg_text<'a>(arg: &'a OsString, context: &str) -> Result<&'a str, CliError> {
    arg.as_os_str()
        .to_str()
        .ok_or_else(|| CliError::new(format!("{context} requires UTF-8 text")))
}

fn is_positional_os_arg(arg: &OsString) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        !arg.as_os_str().as_bytes().starts_with(b"-")
    }

    #[cfg(not(unix))]
    {
        arg.as_os_str()
            .to_str()
            .is_some_and(|arg| !arg.starts_with('-'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Payload, source::LibSourceSpec};
    #[cfg(unix)]
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

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
            "--config-home",
            "/tmp/sim-home",
            "--config-work=/tmp/sim-work",
            "--config-file",
            "/tmp/sim.toml",
            "--config-site=config/runtime",
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
        assert_eq!(boot.config.roots.home, Some(PathBuf::from("/tmp/sim-home")));
        assert_eq!(boot.config.roots.work, PathBuf::from("/tmp/sim-work"));
        assert_eq!(
            boot.config.single_file,
            Some(PathBuf::from("/tmp/sim.toml"))
        );
        assert_eq!(
            boot.config.site_sources,
            vec![sim_kernel::Symbol::qualified("config", "runtime")]
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

    #[cfg(unix)]
    #[test]
    fn load_path_value_preserves_non_utf8_bytes() {
        let parsed = parse_args(vec![
            OsString::from("sim"),
            OsString::from("--load"),
            OsString::from_vec(b"path:/tmp/sim-run-\xff-loader.wasm".to_vec()),
        ])
        .unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        let LibSourceSpec::Path(path) = &boot.loads[0] else {
            panic!("expected path source");
        };

        assert_eq!(
            path.as_os_str().as_bytes(),
            b"/tmp/sim-run-\xff-loader.wasm"
        );
    }

    #[cfg(unix)]
    #[test]
    fn script_path_value_preserves_non_utf8_bytes() {
        let parsed = parse_args(vec![
            OsString::from("sim"),
            OsString::from("--script"),
            OsString::from_vec(b"/tmp/sim-run-\xff-script.sim".to_vec()),
        ])
        .unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        let path = boot.payload.script.as_ref().unwrap();

        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/sim-run-\xff-script.sim");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_text_value_fails_closed() {
        let err = parse_args(vec![
            OsString::from("sim"),
            OsString::from("--codec"),
            OsString::from_vec(b"li\xffsp".to_vec()),
        ])
        .unwrap_err();

        assert_eq!(err.to_string(), "--codec requires UTF-8 text");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_positional_payload_is_preserved() {
        let payload = OsString::from_vec(b"run-\xff".to_vec());
        let parsed = parse_args(vec![OsString::from("sim"), payload.clone()]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };

        assert_eq!(boot.payload.args, vec![payload]);
        assert_eq!(boot.envelope().verb, None);
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
    fn parses_config_report_commands() {
        let parsed = parse_args(["sim", "--config-file=sim.toml", "config", "status"]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(
            boot.config_report,
            Some(ConfigReportRequest {
                kind: ConfigReportKind::Status,
                json: false,
            })
        );

        let parsed = parse_args(["sim", "config", "effective", "sim/cookbook", "--json"]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(
            boot.config_report,
            Some(ConfigReportRequest {
                kind: ConfigReportKind::Effective {
                    lib: sim_kernel::Symbol::qualified("sim", "cookbook"),
                },
                json: true,
            })
        );

        let parsed = parse_args(["sim", "config", "sources", "--json"]).unwrap();
        let CliCommand::Boot(boot) = parsed else {
            panic!("expected boot command");
        };
        assert_eq!(
            boot.config_report,
            Some(ConfigReportRequest {
                kind: ConfigReportKind::Sources,
                json: true,
            })
        );
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
        assert_eq!(
            parse_args(["sim", "--config-file"])
                .unwrap_err()
                .to_string(),
            "--config-file requires a value"
        );
        assert_eq!(
            parse_args(["sim", "--config-work=a", "--config-work=b"])
                .unwrap_err()
                .to_string(),
            "--config-work was provided more than once"
        );
        assert_eq!(
            parse_args(["sim", "config"]).unwrap_err().to_string(),
            "config requires one of: status, effective, sources"
        );
        assert_eq!(
            parse_args(["sim", "config", "effective"])
                .unwrap_err()
                .to_string(),
            "config effective requires exactly one library id"
        );
    }
}
