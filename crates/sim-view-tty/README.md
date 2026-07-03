# sim-view-tty

A loadable terminal view/edit surface for SIM. It projects a `Scene` to text for
a `cli`/`tui` surface and reduces terminal key input to `Intent` values. The
`sim` binary stays a bootloader; this surface is loaded, never baked in.
