# sim-cli-loaders

In one line: The pieces that let SIM pull in outside plug-ins from files or bundles.

## What it gives you

This is the set of loading mechanisms the starting program can switch on when you
want to bring in behavior that lives outside the base build. One mechanism opens a
compiled plug-in file built for your machine. Another opens a portable bundle that
runs the same way anywhere. Either way, you get a clean and consistent path from a
file on disk to live behavior inside a running session. These loaders stay small
and low-level on purpose, so the command surface can add just the loading style
you need without dragging in the whole system. When a loaded plug-in offers a
placement point for later work, this layer records it as an opaque item the rest
of the system can pick up.

## Why you will be glad

- You extend the system by dropping in a file, with no rebuild of the base program.
- You choose a native plug-in or a portable bundle, whichever suits your machine.
- You keep the base program lean, adding a loading style only when you actually need one.

## Where it fits

This layer sits below the command surface and below the full runtime facade. It
holds only the machinery for opening plug-in files and bundles, so the front door
can compose exactly the loading it needs. When a guest plug-in exports a placement
point, this layer stores it under a placement name and leaves the meaning to the
libraries above. It is the quiet bridge between a file you hand over and behavior
that comes alive in your session.
