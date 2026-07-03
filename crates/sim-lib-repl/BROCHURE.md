# sim-lib-repl

In one line: The interactive prompt where you type a line and SIM answers back.

## What it gives you

This is the loadable back-and-forth prompt for SIM. You type one line, the system
reads it, works it out, and prints the answer, then waits for your next line. It
is the hands-on way to explore, try an idea, and see the result right away without
setting up a whole program first. The prompt keeps a clear read-then-answer rhythm
and stays out of your way. It does not carry a language surface or number handling
of its own; instead it expects your session to already have those brought in, so
the very same prompt works over whichever surface you loaded. That keeps it honest
and small: it drives the conversation and lets the loaded pieces supply the actual
meaning of each line you enter.

## Why you will be glad

- You test an idea the moment it occurs to you and read the answer at once.
- You get the same prompt no matter which language surface you chose to load.
- You keep a tight try-and-see loop that makes learning the system far quicker.

## Where it fits

This is a library the starting program hands off to once your session is ready. It
supplies the interactive prompt but leans on the rest of the session for the
language surface, number handling, and evaluation behavior. Because its core read
step is exposed plainly, hosts can exercise the prompt directly in tests. In short
it is the friendly conversational face of a session that other libraries make
capable.
