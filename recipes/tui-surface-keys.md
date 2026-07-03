# Open a value in the TUI surface and drive it with keys

## What it shows

The terminal is a first-class surface, not a fallback. A value is projected to a
Scene, rendered to deterministic terminal text, and driven entirely from the
keyboard. Each key reduces to the SAME Intent vocabulary the Web UI uses, so the
two surfaces behave identically -- the difference is only the surface codec.

## Steps and APIs

1. Project the value to a Scene through the universal surface codec
   (`sim_lib_view`), then render it for the terminal with
   `sim_view_tty::render_scene(&scene, &caps)`, where `caps` is a `tui` or `cli`
   `SurfaceCaps` preset. The output is plain text -- stable for snapshot tests.

2. Map a keypress to an Intent with
   `sim_view_tty::intent_from_key(&key, pane, target, tick)`:

   - Enter  -> `intent/invoke` (activate the focused target)
   - Up/Down -> `intent/select`
   - Left/Right -> `intent/move`
   - a typed Char -> `intent/edit-field`
   - Escape -> `intent/cancel`

   Keys with no mapping (e.g. Backspace) return `None`.

3. Open the command palette with `:`. A `KeyInput::Colon` line is a palette
   filter: `palette_intent_from_colon` runs the shared
   `sim_lib_view::palette` (`filter_commands` + `palette_intent`), so the TUI and
   Web UI resolve the exact same commands.

4. Feed the Intent back through the editor half of the surface codec to fold it
   into a Draft, commit it, and re-render the next Scene.

## Why the surfaces agree

`render_scene` and `intent_from_key` are thin terminal adapters over the shared
view and intent libraries. The palette and Intent set live in `sim_lib_view`,
so a keystroke in the TUI and a click in the browser produce the same Intent and
the same Operation.
