// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// conformance: the bootloader selects prove only for the explicit prove verb.

use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const VERB: &str = sim_lib_world::PROVE_VERB;
const HOST: &str = "lib/prove-command";
const PROVIDER: &str = "platform/site/model";
const LOADER: &str = "loader/static-v1";
const ARTIFACT: &str = "lib/prove-command";

pub(crate) fn with_prove_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_prove_command(command) {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || {
            Box::new(
                sim_lib_world::ProveCommandLib::for_model_load(sim_lib_world::ModelDomainLoad {
                    provider: PROVIDER.to_owned(),
                    loader: LOADER.to_owned(),
                    artifact: ARTIFACT.to_owned(),
                })
                .expect("the bundled prove load names a provider, loader, and artifact"),
            )
        })
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(HOST.to_owned()),
            ],
        )
}

fn is_prove_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == VERB)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_run_core::parse_args;

    #[test]
    fn detects_only_the_prove_verb() {
        assert!(is_prove_command(
            &parse_args(["sim", "prove", "frontier"]).unwrap()
        ));
        for verb in ["world", "roadmap", "index", "proved", "prove-command"] {
            assert!(
                !is_prove_command(&parse_args(["sim", verb]).unwrap()),
                "{verb}"
            );
        }
    }
}
