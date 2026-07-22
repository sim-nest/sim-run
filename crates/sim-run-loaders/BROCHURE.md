# sim-run-loaders

In one line: The pieces that let SIM pull in source files, packs, and outside plug-ins.

## What it gives you

This is the set of loading mechanisms the starting program can switch on when you
want to bring behavior in from outside the base build. It handles readable source
files, compact library packs, compiled plug-ins built for your machine, and
portable bundles that run the same way anywhere. Each path gives the command
surface a consistent route from an artifact to registered behavior inside a
running session. These loaders stay small and low-level on purpose, so the front
door can add just the loading style you need without dragging in the whole system.
When a loaded plug-in offers a placement point, this layer records it as an
opaque item the rest of the system can pick up.

## Why you will be glad

- You extend the system by dropping in source, a pack, or a plug-in, with no rebuild of the base program.
- You choose readable, compact, native, or portable input, whichever suits the job.
- You keep the base program lean, adding a loading style only when you actually need one.

## Where it fits

This layer sits below the command surface and below the full runtime facade. It
holds the machinery for opening source files, packs, plug-in files, and portable
bundles, so the front door can compose exactly the loading it needs. When a guest
plug-in exports a placement point, this layer stores it under a placement name
and leaves the meaning to the libraries above. It is the bridge between an
artifact you hand over and behavior that appears in your session.
