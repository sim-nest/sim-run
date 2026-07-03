# sim-cli

In one line: The `sim` program you launch from a terminal to start a SIM session.

## What it gives you

This is the small starting program that turns the SIM system on. You run one
command, `sim`, and it reads the options you type, then hands control to whatever
behavior you asked for. The program itself stays thin on purpose: it bakes in no
language surface and no built-in tricks. It simply understands how to bring a
library to life and pass your request along. You choose what gets loaded from the
outside, so the same starting program serves many jobs. It stays quiet and
honest: when you ask for something it cannot find, it tells you plainly instead
of guessing. Think of it as the front door to everything else in the system.

## Why you will be glad

- You get one clear entry command instead of a pile of separate tools to learn.
- You stay in charge of what loads, so the program only ever does what you asked.
- You see honest messages when a piece is missing, not silent surprises later.

## Where it fits

This is the outermost layer a person touches. It owns the `sim` name on your
machine and nothing more; the real work lives in the libraries it brings in and
in the command layer just beneath it. Feature builds let it accept plug-in pieces
from a file, a portable bundle, or a shared source, but each of those stays
optional. Everywhere else in SIM assumes this door is how a human first arrives.
