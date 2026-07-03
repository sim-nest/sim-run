# sim-cli-core

In one line: The part that reads your command line and sets up the SIM session.

## What it gives you

This is the reasoning behind the starting program. When you type a command, this
piece figures out what you meant: which language surface to speak, which extra
libraries you want, and where the payload of your request begins. It keeps your
handoff details safe, chooses the right first library to bring in, and then passes
your whole request across to that library. It also turns the result back into a
simple yes-or-no exit signal your shell can read. If you point it at a file, it
loads from there. If you seed a local store ahead of time, it can find pieces
without ever reaching out over a wire. Nothing is assumed for you; every source
is one you named.

## Why you will be glad

- Your typed options are read carefully, so surprises stay rare and explainable.
- You pick the language surface by name, and the right library is brought in for it.
- You can run fully offline from a local store, reaching outward only when you say so.

## Where it fits

This sits just under the `sim` command and does the actual thinking the front
door delegates. It decides which library becomes the boot codec, honors any extra
libraries you loaded by hand, and gives them precedence over the default. The
starting program stays a thin shell around it. Every functional test of the
command surface drives this layer, which makes it the dependable middle of the
whole entry path.
