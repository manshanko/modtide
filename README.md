Basic mod manager that hooks and overlays the Darktide launcher.

Current features:
- toggle/reorder/install mods
- linux/wine support (~~winter~~ year of the linux desktop is coming)

## Use

Download the [latest release] and copy `dwmapi.dll` to `<DARKTIDE>/launcher/` (`<DARKTIDE>/content/launcher/` for gamepass).
When working a `MODS` button will appear in the upper right corner of the Darktide launcher.

[latest release]: https://github.com/manshanko/modtide/releases/latest

The mod list supports:
- selecting multiple mods (click with shift/ctrl)
- double click or `SPACE` toggles selected mods
- click and drag selected mods
- dropdown with right click
- [drag drop mods to install](#installing-mods)

### Installing Mods

Mods can be installed with drag and drop.
It checks the mod layout to determine if it is supported (is `<NAME>/<NAME>.mod` or `binaries`/`mods` present).

modtide currently supports installing from folders and simple `zip`s.
When installing a mod with an unsupported format first extract to a folder then drag drop that folder.
