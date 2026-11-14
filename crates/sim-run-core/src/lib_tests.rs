// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// conformance: the bootloader version line follows the package or a supplied
// binary version, and a direct payload enters the loaded boot path.

use super::*;

#[test]
fn version_line_uses_package_version() {
    assert_eq!(
        version_line(),
        format!("sim {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn version_line_accepts_product_binary_version() {
    assert_eq!(version_line_for("7.8.9"), "sim 7.8.9\n");
}

#[test]
fn direct_payload_enters_loaded_boot() {
    let err = run(["sim", "run"]).unwrap_err();
    assert!(err.to_string().starts_with("no codec 'lisp' available"));
}
