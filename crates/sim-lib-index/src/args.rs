//! Argument parsing for the runtime index command.

use crate::IndexError;

/// Output encoding requested by the command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    /// Stable human-readable text.
    Text,
    /// Stable machine-readable JSON.
    Json,
}

/// Top-level collection selected by `index list`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Collection {
    /// Subjects.
    Subjects,
    /// Anchors.
    Anchors,
    /// Surfaces.
    Surfaces,
    /// Specimens.
    Specimens,
    /// Features.
    Features,
    /// Routes.
    Routes,
    /// Edges.
    Edges,
    /// Every collection.
    All,
}

impl Collection {
    /// Parses a collection label.
    pub fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "subjects" => Ok(Self::Subjects),
            "anchors" => Ok(Self::Anchors),
            "surfaces" => Ok(Self::Surfaces),
            "specimens" | "examples" => Ok(Self::Specimens),
            "features" => Ok(Self::Features),
            "routes" => Ok(Self::Routes),
            "edges" => Ok(Self::Edges),
            "all" => Ok(Self::All),
            _ => Err(IndexError::new(format!(
                "unknown index collection: {value}"
            ))),
        }
    }

    /// Stable collection label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subjects => "subjects",
            Self::Anchors => "anchors",
            Self::Surfaces => "surfaces",
            Self::Specimens => "specimens",
            Self::Features => "features",
            Self::Routes => "routes",
            Self::Edges => "edges",
            Self::All => "all",
        }
    }
}

/// Parsed `sim index` command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexCommand {
    /// Render command help.
    Help,
    /// List one top-level collection.
    List {
        /// Collection to render.
        collection: Collection,
        /// Output encoding.
        output: OutputMode,
    },
    /// Show one id.
    Show {
        /// Id to show.
        id: String,
        /// Output encoding.
        output: OutputMode,
    },
    /// Search the graph.
    Find {
        /// Query terms and filters.
        query: crate::Query,
        /// Output encoding.
        output: OutputMode,
    },
    /// Trace one id through adjacent graph facts.
    Trace {
        /// Id to trace.
        id: String,
        /// Output encoding.
        output: OutputMode,
    },
    /// List examples attached to a feature id.
    Examples {
        /// Feature id to inspect.
        feature: String,
        /// Output encoding.
        output: OutputMode,
    },
    /// Find best-use routes for a task.
    Route {
        /// Task text to route.
        task: String,
        /// Output encoding.
        output: OutputMode,
    },
}

/// Parses a `sim index` payload argument list.
pub fn parse_index_args(args: &[String]) -> Result<IndexCommand, IndexError> {
    let args = strip_verb(args);
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(IndexCommand::Help);
    }
    match args[0].as_str() {
        "list" => parse_list(&args[1..]),
        "show" => parse_show(&args[1..]),
        "find" => parse_find(&args[1..]),
        "route" => parse_route(&args[1..]),
        "trace" => parse_trace(&args[1..]),
        "examples" => parse_examples(&args[1..]),
        other => Err(IndexError::new(format!("unknown index verb: {other}"))),
    }
}

fn strip_verb(args: &[String]) -> &[String] {
    if args.first().is_some_and(|arg| arg == "index") {
        &args[1..]
    } else {
        args
    }
}

fn parse_list(args: &[String]) -> Result<IndexCommand, IndexError> {
    let mut output = OutputMode::Text;
    let mut collection = Collection::All;
    for arg in args {
        match arg.as_str() {
            "--json" => output = OutputMode::Json,
            value if value.starts_with('-') => {
                return Err(IndexError::new(format!("unknown list option: {value}")));
            }
            value => collection = Collection::parse(value)?,
        }
    }
    Ok(IndexCommand::List { collection, output })
}

fn parse_show(args: &[String]) -> Result<IndexCommand, IndexError> {
    let (id, output) = parse_id_and_output("show", args)?;
    Ok(IndexCommand::Show { id, output })
}

fn parse_trace(args: &[String]) -> Result<IndexCommand, IndexError> {
    let (id, output) = parse_id_and_output("trace", args)?;
    Ok(IndexCommand::Trace { id, output })
}

fn parse_examples(args: &[String]) -> Result<IndexCommand, IndexError> {
    let (feature, output) = parse_id_and_output("examples", args)?;
    Ok(IndexCommand::Examples { feature, output })
}

fn parse_route(args: &[String]) -> Result<IndexCommand, IndexError> {
    let mut output = OutputMode::Text;
    let mut task = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--json" => output = OutputMode::Json,
            value if value.starts_with('-') => {
                return Err(IndexError::new(format!("unknown route option: {value}")));
            }
            value => task.push(value.to_owned()),
        }
    }
    if task.is_empty() {
        return Err(IndexError::new("route requires task text"));
    }
    Ok(IndexCommand::Route {
        task: task.join(" "),
        output,
    })
}

fn parse_id_and_output(verb: &str, args: &[String]) -> Result<(String, OutputMode), IndexError> {
    let mut output = OutputMode::Text;
    let mut id = None;
    for arg in args {
        match arg.as_str() {
            "--json" => output = OutputMode::Json,
            value if value.starts_with('-') => {
                return Err(IndexError::new(format!("unknown {verb} option: {value}")));
            }
            value => {
                if id.replace(value.to_owned()).is_some() {
                    return Err(IndexError::new(format!("{verb} accepts exactly one id")));
                }
            }
        }
    }
    let Some(id) = id else {
        return Err(IndexError::new(format!("{verb} requires an id")));
    };
    Ok((id, output))
}

fn parse_find(args: &[String]) -> Result<IndexCommand, IndexError> {
    let mut output = OutputMode::Text;
    let mut query = crate::Query::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--json" => output = OutputMode::Json,
            "--audience" => query.audience = Some(take_value(args, &mut i, "--audience")?),
            "--surface-kind" => {
                query.surface_kind = Some(take_value(args, &mut i, "--surface-kind")?)
            }
            "--language" => query.language = Some(take_value(args, &mut i, "--language")?),
            "--grammar" => query.grammar = Some(take_value(args, &mut i, "--grammar")?),
            "--repo" => query.repo = Some(take_value(args, &mut i, "--repo")?),
            "--package" => query.package = Some(take_value(args, &mut i, "--package")?),
            "--anchor" => query.anchor = Some(take_value(args, &mut i, "--anchor")?),
            value if value.starts_with('-') => {
                return Err(IndexError::new(format!("unknown find option: {value}")));
            }
            value => query.terms.push(value.to_owned()),
        }
        i += 1;
    }
    if query.terms.is_empty() && query.is_unfiltered() {
        return Err(IndexError::new("find requires a term or filter"));
    }
    Ok(IndexCommand::Find { query, output })
}

fn take_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, IndexError> {
    *i += 1;
    args.get(*i)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or_else(|| IndexError::new(format!("{flag} requires a value")))
}

#[cfg(test)]
mod tests {
    use super::{IndexCommand, OutputMode, parse_index_args};

    #[test]
    fn parses_find_terms_before_flag() {
        let args = vec![
            "index".to_owned(),
            "find".to_owned(),
            "codec".to_owned(),
            "--audience".to_owned(),
            "code".to_owned(),
            "--json".to_owned(),
        ];

        let command = parse_index_args(&args).unwrap();

        let IndexCommand::Find { query, output } = command else {
            panic!("expected find");
        };
        assert_eq!(query.terms, ["codec"]);
        assert_eq!(query.audience.as_deref(), Some("code"));
        assert_eq!(output, OutputMode::Json);
    }

    #[test]
    fn parses_route_task_with_json_flag() {
        let args = vec![
            "index".to_owned(),
            "route".to_owned(),
            "write".to_owned(),
            "a".to_owned(),
            "parser".to_owned(),
            "--json".to_owned(),
        ];

        let command = parse_index_args(&args).unwrap();

        let IndexCommand::Route { task, output } = command else {
            panic!("expected route");
        };
        assert_eq!(task, "write a parser");
        assert_eq!(output, OutputMode::Json);
    }
}
