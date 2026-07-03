# sim-view-tty

In one line: The library that draws SIM in your terminal and reads your keystrokes.

## What it gives you

This is the terminal face of SIM. It takes a scene the system wants to show and
paints it as plain text that fits your terminal, and it turns the keys you press
into clear intentions the system can act on. Both directions are steady and
predictable, so what you see and what you type behave the same way every run. It
offers two settings: a keyboard-only plain view for a simple terminal, and a
richer view that also understands pointer input and a wider palette. The terminal
is treated as one kind of display among several, not a special built-in mode, so
the starting program stays a plain front door while this piece handles everything
about showing and reading in text.

## Why you will be glad

- You work in a normal terminal and still get a clean, readable view of the system.
- You pick a simple keyboard view or a richer one that also reads pointer input.
- You get steady, repeatable drawing and input, which makes sessions easy to trust.

## Where it fits

This is a library the session loads when a terminal is the display you want. It
draws outgoing scenes to text and folds incoming keystrokes into plain intentions,
leaving argument reading and process control to the starting program. Because both
directions are pure, the whole surface can be checked without a real terminal
attached. It is the piece that lets SIM meet you where you already are: at a text
prompt in a window.
