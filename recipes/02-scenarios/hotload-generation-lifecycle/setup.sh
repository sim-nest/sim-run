#!/bin/sh
set -eu

cargo test -p sim-lib-hotload surface::tests
