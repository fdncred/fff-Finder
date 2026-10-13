<img src="assets/app-icon.svg" alt="fff-Finder icon" width="80" height="80">

# fff-Finder

A Finder-style file browser with instant fuzzy search across your whole
computer, written in Rust. Search uses [fff-search](https://crates.io/crates/fff-search),
the UI is built with [GPUI Kit](https://github.com/longbridge/gpui-kit), and
Nushell is built into both sides. Your shell can query the index the app keeps
live, and the app runs Nushell on the files you select: pipelines, filters,
rename closures, and your own actions.

This README is an overview. **[The manual](docs/manual.md) documents every
feature**: each view, menu item, keyboard shortcut, setting, `settings.json`
field, `fff` command, and command-line flag.

![Icon view in dark mode](docs/images/icons-dark.png)

## What it does

- Indexes your home folder (and `/Applications` on macOS) in the background and
  keeps the index live with filesystem watchers. On the development Mac that is
  about 670,000 files indexed in 4 to 6 seconds, and a search over all of them
  returns in 30 to 110 ms.
- Searches by name with typo tolerance, or by file contents. You can narrow a
  search to the current folder, a kind (folder, document, image, movie, audio,
  code, archive), an extension, a glob, or a git status.
- Browses folders in four views (icons, list, columns, gallery), with a sidebar,
  back and forward history, a clickable path bar, and a status bar. Gallery
  shows a large preview of the selected item above a strip of everything in
  the folder; arrow keys move along the strip. As in Finder, typing a name
  while the files have focus jumps to the first item that starts with it.
- Previews files in the gallery, the column view, and Quick Look (Space).
  Images render as images. CSV, JSON, TOML, YAML, SQLite and other structured
  files open through Nushell's `open` and show as tables, and `.nu` scripts are
  colored by `nu-highlight` with your own `color_config` (plus gray comments,
  which `nu-highlight` leaves plain). The small and large
  "A" buttons in Quick Look (or Cmd+Plus and Cmd+Minus) change the text size
  of text, code, and table previews, and fff-Finder remembers it. The gallery
  uses the same size; Cmd+0 or the size label sets it back to 11.5 pt.
- Handles everyday file work: open, open in your editor, show in
  Finder/Explorer, copy paths, copy as a Nushell list, rename one item or many,
  new folder, move to Trash, and dragging files out to other apps (macOS and
  Wayland; GPUI doesn't support drag-out on Windows or X11). A text file no
  app claims, such as a `.nu` script on a fresh Mac, opens in your editor
  instead of failing.
- Has a text size for the whole window (Settings › Text size, 80% to 200%),
  and separate text sizes for the Nushell panel and Quick Look.
- On macOS it looks like Finder. File and folder icons are the ones Finder
  shows, drawn by macOS, and the sidebar uses Finder's sidebar icons. The
  colors were measured from macOS's own light and dark appearances, and the
  accent and selection colors follow System Settings.

![Search results](docs/images/search-dark.png)

## Nushell integration

### The `fff` module

`nu/fff.nu` is a Nushell module. Load it from your config:

```nu
use /path/to/fff-Finder/nu/fff.nu *
fff keybindings      # Ctrl+T inserts picked paths, Alt+C picks a folder to cd into
```

Most of its commands ask the running fff-Finder for answers, so nothing is
scanned in the shell and results come from the same live index the window uses.

```nu
fff find budget --kind document        # name search, ranked like the window
fff find main.rs --here --files        # only under the current folder
fff search TODO FIXME --context 1      # content search, several patterns
fff search 'fn \w+_test' --regex --scope ~/src
fff files | length                     # every indexed file
fff status                             # roots, file counts, scanning or not
fff watch --timeout 30sec              # stream file changes from the index
```

`fff find` returns `name`, `type`, `size` (a filesize), `modified` (a date),
`git` (when the file has a git status), `path`, and `score`.

These commands work in both directions with the window:

```nu
fff selection | get path               # what's selected in the front window
fff selection --all                    # everything it shows
ls **/*.log | where size > 10mb | fff show --title 'Big logs'
fff pick *.csv | open $in              # choose a file in the window, use it here
fff gui ~/src --query readme           # open a window, optionally searching
```

`fff show` takes paths, `ls` or `glob` output, or any table with a `path` or
`name` column, and opens the results in the window like a search. It starts
fff-Finder if it isn't running. `fff insert`, `fff cd`, and `fff edit` build on
`fff pick`; `fff edit` uses `$env.config.buffer_editor`, then `$VISUAL`, then
`$EDITOR`. `fff open` fuzzy-finds a document and opens it as data.

### The local socket

The app answers requests over a Unix socket at `run/fff-Finder.sock` in its
data folder (`~/Library/Application Support/fff-Finder` on macOS,
`~/.local/share/fff-Finder` on Linux). Only your user can open it. On Windows
it listens on 127.0.0.1 and requires a random token, both written to
`run/server.json`. The endpoints are plain HTTP with JSON, so Nushell can call
them directly:

```nu
let sock = ($nu.data-dir | path dirname | path join fff-Finder run fff-Finder.sock)
http get --unix-socket $sock 'http://localhost/v1/find?query=readme&limit=5'
```

[docs/protocol.md](docs/protocol.md) describes every endpoint: status, find,
files, dirs, content search, watch, selection, show, and open.

Nushell's built-in `idx` commands keep their own index in the shell. A
proposed Nushell change adds `idx connect`, which points `idx find`, `idx
search`, `idx files`, and the rest at fff-Finder's socket instead, with the same
output. It isn't in a Nushell release yet.

### Inside the app

**The Nushell panel** (Ctrl+`) runs a pipeline with the selected files, or
everything shown, as `$in`. Each row has `name`, `path`, `type`, `kind`,
`size`, and `modified`, so `where size > 1mb | sort-by modified` works as you'd
expect, and the result shows as a table. **Clear** empties the pipeline and
brings back the examples.

![Nushell pipeline panel](docs/images/nushell-dark.png)

**Filters** (Cmd+Alt+F, or the funnel button while searching) narrow the folder or the search
results with a Nushell `where` condition, such as
`size > 10mb and modified > 2024-01-01`. Start with `|` to use a whole pipeline
instead: `| sort-by size --reverse | first 20`. The status bar counts how many
items pass. The filter bar's Save button keeps the search, scope, and filter
in the sidebar under Smart Folders.

**Batch rename.** Select several items and press F2 (or choose "Rename N
Items…") to rename them with a closure. Each item has the usual columns plus
`stem`, `ext`, and a 1-based `index`:

```nu
{|f| $f.name | str downcase }
{|f| $"($f.index | fill --alignment right --character '0' --width 3) ($f.name)" }
```

The dialog previews every new name and flags clashes before anything changes.
Renaming happens in two steps through temporary names, so swaps like
`a → b, b → a` work, and a failure puts back the names that already changed.

**Your own actions.** Commands exported from `actions.nu` in the settings
folder show up under "Nushell Actions" in the context menu and in the toolbar's
Nushell menu. Each one receives the selected items as a table on `$in`, and
whatever it returns shows in the Nushell panel. The first run writes some
starters: count lines, total size, Markdown links, find duplicates, and
archive to `.tar.gz`. A newer fff-Finder updates the starters if you never
edited the file. "Edit Actions…" opens the file, and fff-Finder reloads it
when you save.

```nu
# Lines of code per selected file.
export def "count lines" []: table -> table {
    $in | where type == file | each {|f| {name: $f.name, lines: (open --raw $f.path | lines | length)} }
}
```

**Your Nushell environment.** With "Use Nushell's file colors and editor" on in
Settings, fff-Finder reads `$env.LS_COLORS` and `$env.config.buffer_editor` from
a `nu --login` session. File names are drawn the way `ls` draws them (with
Nushell's default colors when `LS_COLORS` isn't set), backgrounds and
underlines included, so README is black on yellow as in your terminal, and
"Open in Editor" uses your editor. Terminal editors such as Helix or Neovim open in the terminal you
chose for "Open Nushell Here".

"Open Nushell Here" opens `nu` in the folder, in the terminal you choose in
Settings (see [Terminal](#terminal)), and "Copy as Nushell List" copies the
selection as NUON.

**`fff-Finder --pick`** turns the window into a file chooser for any script.
Return or double-click prints a JSON array of absolute paths and exits 0, and
Escape exits 1. `fff pick` is a thin wrapper around it.

The build, install, packaging, and screenshot scripts in `scripts/` are Nushell
too.

## Install

You need Rust 1.92 or newer and Nushell 0.116.

- macOS: Xcode Command Line Tools (`xcode-select --install`).
- Linux: `gcc g++ clang libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev libssl-dev libzstd-dev`, plus a Vulkan driver.
  The index watches folders with inotify, one watch per folder, and a big home
  folder can exceed the default limit. Raise it with
  `sudo sysctl fs.inotify.max_user_watches=1048576`.
- Windows: Visual Studio 2022 Build Tools with the C++ workload.

```nu
nu scripts/install.nu            # release build into ~/.cargo/bin
```

`nu scripts/install.nu --dest ~/bin` installs somewhere else. The script prints
the `use` line to add to your config for the `fff` module.

To package the app instead:

```nu
nu scripts/bundle.nu             # macOS: dist/fff-Finder.app, Linux: a folder with a .desktop file
nu scripts/bundle.nu --install   # Linux: also install the desktop entry for your user
```

The bundle includes `fff.nu`, and the module finds the app in `/Applications`
or `~/Applications`, on `PATH`, or through `$env.FFF_FINDER_BIN`.

macOS asks before an app reads Desktop, Documents, or Downloads. To index those
and other protected folders without prompts, give fff-Finder Full Disk Access in
System Settings > Privacy & Security, then reopen it.

Starting fff-Finder while it's already running opens a window in the running
copy, which keeps one index and one socket. On macOS it keeps running after
you close the last window, like Finder, so the shell commands stay fast.

## Using it

### Search syntax

| Type            | Meaning                                                 |
| --------------- | ------------------------------------------------------- |
| `budget report` | Fuzzy words, typo tolerant. Every word must match.      |
| `*.pdf *.md`    | Extensions. Several are ORed.                           |
| `src/**/*.rs`   | A glob against the path inside the search location.     |
| `/notes/ todo`  | Only paths inside a folder named `notes`.               |
| `git:modified`  | Git status: `modified`, `untracked`, `staged`, `clean`. |
| `!*.log !test`  | Exclude an extension, glob, folder, or text.            |

Searches start 50 ms after you stop typing. Return searches right away, and Down
Arrow moves from the search field into the results. When a word matches files
exactly, typo-only matches are dropped, and file-name matches rank above
matches that only appear in a folder name. Cmd+/ shows this table in the app.

### Keyboard

| Keys (Cmd on macOS, Ctrl elsewhere) | Action                        |
| ----------------------------------- | ----------------------------- |
| Cmd+F                               | Search                        |
| Cmd+Alt+F                           | Filter with Nushell           |
| Cmd+1 to Cmd+4                      | Icons, list, columns, gallery |
| Return, Cmd+Down, Cmd+O             | Open                          |
| Space, Cmd+Y                        | Quick Look                    |
| Cmd+Plus, Cmd+Minus, Cmd+0          | Quick Look text size, reset   |
| Cmd+[ and Cmd+]                     | Back and forward              |
| Cmd+Up                              | Enclosing folder              |
| Cmd+Shift+H, Cmd+Shift+F            | Home, Recents                 |
| Cmd+Shift+G                         | Go to folder                  |
| Cmd+Alt+C                           | Copy path                     |
| Cmd+Shift+C                         | Copy as Nushell list          |
| Ctrl+`                              | Nushell panel                 |
| Cmd+Alt+T                           | Open Nushell here             |
| Cmd+Shift+N                         | New folder                    |
| F2                                  | Rename (several: batch rename) |
| Cmd+Backspace                       | Move to Trash                 |
| Cmd+Shift+.                         | Show hidden files             |
| Cmd+R                               | Refresh                       |
| Cmd+Alt+S                           | Toggle the sidebar            |
| Cmd+Shift+D                         | Switch light and dark         |
| Cmd+,                               | Settings                      |

### Settings

Settings live in `settings.json` in the platform config folder
(`~/Library/Application Support/fff-Finder` on macOS, `~/.config/fff-Finder` on
Linux, `%APPDATA%\fff-Finder` on Windows), next to `actions.nu`. Set
`FFF_FINDER_CONFIG_DIR` to use a different folder, and `FFF_FINDER_DATA_DIR`
for the data folder that holds the socket, frecency, and cached icons. The
Settings window edits:

- search locations (including "Index Entire Disk");
- fff-search's options: following symlinks, watching for changes, git
  recency ranking, content indexing, preloading, letter case, the file size
  and time limits for content search, threads, and how many results show;
- appearance, hidden files, and the window's text size;
- the Nushell environment option, the Nushell text size, and the terminal.

Quick Look saves its text size there too. The manual lists
[every field](docs/manual.md#settingsjson).

Settings from before the rename, in a `fffFinder` folder, move to the new
folder the first time fff-Finder starts.

### Terminal

Settings lists the terminals installed on the computer, and "Open Nushell Here"
uses the one you pick. "Try It" opens it in the current folder. "Automatic"
uses the first of these that is installed:

- macOS: Ghostty, iTerm2, WezTerm, kitty, Alacritty, Rio, then Terminal.
- Windows: Windows Terminal, WezTerm, Alacritty, then a plain console window.
- Linux: `$TERMINAL` if it is set, then Ghostty, WezTerm, kitty, Alacritty, Rio,
  foot, GNOME Terminal, Konsole, Xfce Terminal, then any other terminal.

The choice is saved per operating system, so one settings file can say Ghostty
on macOS and Windows Terminal on Windows:

```json
"terminals": {
  "macos": { "app": "ghostty" },
  "windows": { "app": "windows-terminal" },
  "linux": { "app": "custom", "command": "foot --working-directory={dir} {nu}" }
}
```

"Custom Command" runs any command. `{dir}` becomes the folder and `{nu}` the
path to `nu`, and quotes group words that contain spaces.

Terminals are found the way each system records them. On macOS fff-Finder reads
the `Info.plist` of every app in the Applications folders and of every app the
search index finds elsewhere, and an app that adds a "New … Window Here"
service and opens shell scripts counts even if fff-Finder doesn't know it. On
Linux it reads `.desktop` entries in the `TerminalEmulator` category, programs
on `PATH`, and AppImages in the index. Flatpak terminals are skipped because
the sandbox can't see the host's `nu`. On Windows it checks the install
folders, `PATH`, and the index, which finds portable and Scoop installs.

Ghostty on macOS opens a new window in the running app through AppleScript, and
macOS asks once whether fff-Finder may control Ghostty. If Ghostty isn't
running, or you decline, a new Ghostty instance opens instead. fff-Finder avoids
Ghostty's `-e` flag because Ghostty asks for confirmation every time it is used.

## How it's built

```text
crates/fff-Finder-core   search engine wrapper, file model, listing, places, settings, terminals
docs/manual.md           the user manual
src/                     the GPUI Kit app (src/finder/ holds the window and its views)
src/server/              the local socket server and its client
src/macos.rs             Finder's icons and colors from AppKit (macOS only)
nu/fff.nu                the `fff` Nushell module
assets/actions.nu        the starter Nushell actions
docs/protocol.md         the socket protocol
scripts/*.nu             build, install, bundle, screenshots
examples/screenshot.rs   renders the real window headlessly to a PNG
```

The engine keeps one fff index per search root and searches them in parallel.
Folders outside every root get a temporary index the first time you search
them. fff ranks results, then fff-Finder re-ranks them the way Finder users
expect: name matches first, shallower paths ahead of deep ones, and nothing from
inside `.app` or `.framework` bundles.

File icons are drawn once by macOS and cached as PNGs in the data folder, one
per extension and one per app or special folder. Drawing an app's icon the
first time takes about 10 ms, so a window full of new ones fills in over a few
frames and the window keeps responding.

## Development

```nu
nu scripts/build.nu --test --lint    # build, test, clippy
nu scripts/screenshots.nu            # regenerate docs/images (macOS)
cargo run -- ~/src --query readme    # run from source
```

The UI tests open the real window headlessly and use it with mouse clicks and
key presses:

- `tests/controls.rs` clicks every control and checks what each click changed:
  toolbar buttons and menus, the search field and scope bar, the filter bar,
  sidebar rows and Smart Folders, the path bar, the icon-size slider, list
  headers, items in all four views, context menus and Nushell actions, every
  button and field in every dialog (batch rename included), the Nushell panel,
  and the picker bar.
- `tests/finder.rs` presses every keyboard shortcut except Quit and New Window,
  which belong to the app rather than the window, and types to select.
- `tests/server.rs` talks to the socket server, and `tests/nu_module.rs` runs
  `fff.nu` in a real `nu` against it.

A step that runs longer than 20 seconds fails the run and names the step, so a
hang shows up as a failure instead of a `cargo test` that never ends. Opening a
file or showing it in Finder is recorded instead of performed, and nothing is
moved to the Trash.

`examples/drive.rs` replays the same kind of steps in the visible window
against your real index and prints the window's state after each one. It
never writes your settings.

```nu
cargo run --example drive -- key:cmd-, click:show-hidden key:escape key:cmd-shift-g 'type:~/src' key:enter
```

`examples/screenshot.rs` renders the window to a PNG with Metal, so a UI change
can be checked without opening it. `examples/macos_colors.rs` prints the
macOS colors the themes are based on, in light and dark.

`examples/terminals.rs` lists the terminals found and what Automatic picks.
With `--verify` it opens each one with a short script in place of `nu` that
records the folder it started in, then closes it. Terminal and iTerm2 keep
their windows open, so it checks them only when you name them.

```nu
cargo run --example terminals -- --verify
```

If the window ever stops responding, run it with `FFF_FINDER_WATCHDOG=1`. After
three seconds without a response it saves every thread's stack to
`hang-<time>.txt` in the data folder and prints the path. Other platforms print
the process ID to attach a debugger to.

## Limits

fff-search 0.11 decides some of the behavior:

- Hidden files are indexed only inside git repositories.
- Searches can't filter by size or date. The Nushell filter bar can, on the
  results a search returns.
- The first scan has to finish before searches return results. The status bar
  and sidebar show progress, and the search re-runs on its own when the scan
  is done.

Compared with FinderSearch, fff-Finder leaves out tags, sharing, copy and paste
of files, undo, ZIP tools, and tabs. Tags and sharing are macOS services, and
the rest is file-mutation work that sits outside a search app. Nushell actions
cover some of it; the starters include a `.tar.gz` archiver.

Finder's own icons and the system accent color are macOS only. Linux and
Windows use the bundled icons and the measured Finder colors.

Only the macOS build has been run and tested so far. The Linux and Windows code
is written against each platform's APIs, and `.github/workflows/ci.yml` builds and
tests all three, but expect rough edges there until it gets hands-on use.
