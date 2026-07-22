//! A reusable bootloader for a binary that ships statically-linked libraries and
//! boots them through the same [`LoadSession`] machinery the `sim` binary uses.
//!
//! Every product binary composes a [`Bootloader`], registers its serve library as a
//! host factory, and dispatches the library's `cli/main/<verb>` entrypoint -- the
//! exact path `sim --load host:<lib> <verb>` follows. A binary owns the one canonical
//! runtime bootstrap through this type and constructs no `Cx` of its own.
//!
//! The pattern mirrors the interactive REPL boot (`sim repl`): a host-registered
//! library exports a `cli/main/<verb>` function whose [`Callable`] runs the (possibly
//! long-lived, blocking) serve loop and returns a truthy value at shutdown, which the
//! bootloader maps to the process exit code.

use std::ffi::OsString;

use sim_kernel::{CapabilityName, Cx, Lib, Symbol};

use crate::{
    CliError, LibSourceSpec, LoadSession, RuntimeConfigState, parse_args, run_command_with_session,
};

/// A thin bootloader over [`LoadSession`] for a single-library product binary.
///
/// ```no_run
/// # use sim_run_core::Bootloader;
/// # struct MyServeLib;
/// # impl MyServeLib { fn new() -> Self { Self } }
/// # impl sim_kernel::Lib for MyServeLib {
/// #     fn manifest(&self) -> sim_kernel::LibManifest { unimplemented!() }
/// #     fn load(&self, _: &mut sim_kernel::LoadCx, _: &mut sim_kernel::Linker<'_>)
/// #         -> sim_kernel::Result<()> { Ok(()) }
/// # }
/// // `my-server ARGS...` boots exactly like `sim serve ARGS...`, dispatching the
/// // library's `cli/main/serve` entrypoint. No Cx::new in the binary.
/// let code = Bootloader::standard()
///     .host_verb("serve", "lib/my-server", || Box::new(MyServeLib::new()))
///     .run(std::iter::once("serve".into()).chain(std::env::args_os().skip(1)))?;
/// std::process::exit(code);
/// # Ok::<(), sim_run_core::CliError>(())
/// ```
pub struct Bootloader {
    session: LoadSession,
}

impl Bootloader {
    /// The standard boot session: the in-process host loader only, matching the
    /// default `sim` binary. Add libraries with [`Bootloader::host_verb`].
    pub fn standard() -> Self {
        Self {
            session: LoadSession::new(),
        }
    }

    /// Registers a statically-linked library under `name` and makes it the default
    /// source for `verb`, so a bare `<verb> ARGS...` dispatches to the library's
    /// `cli/main/<verb>` entrypoint with no explicit `--load`.
    ///
    /// The library must be a [`LibTarget::HostRegistered`](sim_kernel::LibTarget)
    /// lib that exports a `cli/main/<verb>` function (see
    /// [`crate::cli_main_entrypoint_symbol`]).
    pub fn host_verb<F>(self, verb: &str, name: &str, factory: F) -> Self
    where
        F: Fn() -> Box<dyn Lib> + Send + Sync + 'static,
    {
        Self {
            session: self
                .session
                .with_host_factory(name.to_owned(), factory)
                .with_default_verb_sources(
                    verb.to_owned(),
                    vec![LibSourceSpec::Host(name.to_owned())],
                ),
        }
    }

    /// Registers a statically-linked library under `name`, makes it the default
    /// source for `verb`, and reads the supplied config library ids before the
    /// host library is instantiated.
    pub fn host_verb_with_config<F>(
        self,
        verb: &str,
        name: &str,
        config_libs: Vec<Symbol>,
        factory: F,
    ) -> Self
    where
        F: Fn(&RuntimeConfigState) -> Box<dyn Lib> + Send + Sync + 'static,
    {
        Self {
            session: self
                .session
                .with_host_factory_with_config(name.to_owned(), factory)
                .with_default_verb_sources(
                    verb.to_owned(),
                    vec![LibSourceSpec::Host(name.to_owned())],
                )
                .with_default_verb_config_libs(verb.to_owned(), config_libs),
        }
    }

    /// Registers a statically-linked library under `name` WITHOUT binding it to a
    /// verb -- for a supporting library the served verb needs, most commonly the boot
    /// codec (register it under `codec/<name>` and pass `--codec <name>` so the boot
    /// resolves the host codec instead of the default).
    pub fn host_lib<F>(self, name: &str, factory: F) -> Self
    where
        F: Fn() -> Box<dyn Lib> + Send + Sync + 'static,
    {
        Self {
            session: self.session.with_host_factory(name.to_owned(), factory),
        }
    }

    /// Grants a capability the served library requires (for example a transport or
    /// tool-call capability).
    pub fn with_capability(self, capability: CapabilityName) -> Self {
        Self {
            session: self.session.with_capability(capability),
        }
    }

    /// Installs context-level runtime support into the boot `Cx` before dispatch
    /// (e.g. a codec, an eval policy, a supporting lib). Concrete serve behavior
    /// stays in the loaded library.
    pub fn with_context<F>(self, configure: F) -> Self
    where
        F: FnOnce(&mut Cx),
    {
        Self {
            session: self.session.with_context(configure),
        }
    }

    /// Boots the runtime, applies the registered libraries, and dispatches `args`
    /// (typically the process arguments). A blocking serve verb runs to completion
    /// here; its returned value becomes the process exit code.
    pub fn run<I, S>(self, args: I) -> Result<i32, CliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut session = self.session;
        let command = parse_args(args)?;
        run_command_with_session(command, &mut session)
    }
}
