# sim-cli-core

In one line: Give command libraries and their dispatcher one shared naming
agreement without pulling a bootloader into every command library.

## What it gives you

A command needs a name that its dispatcher recognizes. This library supplies
that agreement as plain data, so independently built commands describe their
entry points consistently. Describing a command does not start it, load its
implementation, open a connection, or reserve a device. The description stays
separate from the runtime that decides whether it can actually be called.

## Why you will be glad

You can build and check command descriptions without installing a complete
session. Library authors and dispatcher authors share the same convention,
making mismatched names easier to catch in small tests. Dependencies reflect
the responsibility being used: naming does not require network or audio host
implementations merely because those services support other commands.

## Where it fits

This is the contract between command libraries and the bootloader. It owns
names and descriptions, while loaded libraries own behavior and the bootloader
owns selection and execution. A description is not a permission or a promise
that an implementation is available; those decisions remain with the caller's
actual runtime and its authority checks.
