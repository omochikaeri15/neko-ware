# Neko-Arc
Switch assets are stored under a 64-bit hash of their full asset path (`Data/Image/{name}.png.bntx` for art, `Data/CsvFiles/{name}.csb` for `.csv` and `.tsv` tables), so an unpacked archive is a folder of hex names until it gets a name source to match against.

### DUMP
`neko-arc dump <FILE | DIR>...`

Pulls every `.arc` straight out of an `.nsp`. They land in a folder named after the NSP in the working directory, under the same `Archive/` path the game reads them from, ready to hand to `decrypt`.

`--extra` (`-e`) dumps everything else instead: the sound, fonts, offline help pages, and rumble patterns that ship beside the archives rather than inside them. They are not encrypted beyond the NSP itself, so they come out exactly as the game reads them, under their own `Data/` paths.

Everything inside an NSP is encrypted against the console, so it needs a `prod.keys` dumped from your own Switch. One sitting beside the `neko-arc` binary is picked up on its own, following a symlink back to wherever the real binary lives; `--keys` (`-k`) `<FILE>` points at one anywhere else instead. Either way, the same keys cover every NSP in the run.

An update is a patch that only makes sense laid over its base game, so whenever one is found it is applied, and what comes out is the set of archives the game actually loads once it is installed. Without an update, the base archives are dumped as they shipped.

The update does not have to live in the same NSP as its base. Every NSP named in a run is read first and each update is paired with its base game by title ID, the way an emulator keeps the two installed side by side, so `neko-arc dump "Game [v0].nsp" "Game [Update].nsp"` yields one folder, named after the base NSP, with the update already applied. An update named without its base is reported and skipped, and a second base or update for a game already found is ignored with a warning, so name only the update you want.

Accepts `--file` (`-f`), repeatable, and `--dir` (`-d`) `<DIR> <LEVEL>` the same way `decrypt` does: walking a directory picks up `.nsp` files only, and one bad NSP is reported and skipped rather than ending the run. `--output` (`-o`) behaves as it does for `decrypt`, too.

### RIP
`neko-arc rip <NSP>...`

Takes an `.nsp` all the way to named, converted files in one pass: everything `dump`, `decrypt`, `decode --rename`, and `convert` would do, run back to back. Only `.nsp` files are accepted, and naming anything else ends the run before it starts.

Nothing in between ever touches the disk. Each archive member is read straight out of the NSP, inflated, named, and converted in memory, so the only files written are the finished ones. They land in a folder named after the NSP in the working directory, with a folder per archive inside (`Data/`, `Data_en/`, `Patch001/`, and so on), and match what the four commands would leave you with byte for byte. A member the dictionary has no name for keeps its hash and is converted the way `convert` handles any other hashed file.

The keys and the dictionary are both checked before any work begins, and the run stops if either is missing. `--keys` (`-k`) `<FILE>` and `--dictionary` (`-c`) `<FILE>` point at them the same way they do for `dump` and `decode`; otherwise the `prod.keys` and `dictionary.csv` sitting beside the `neko-arc` binary are used. Those are the only two flags it takes.

Updates are paired and applied exactly as they are for `dump`, so name the update NSP alongside its base. Only the archives are ripped; the rest of the NSP is left to `dump --extra`. `fix` is never run, so textures come out premultiplied, just as `convert` leaves them.

### DECRYPT
`neko-arc decrypt <FILE | DIR>...`

Extracts every member of an `.arc` into a folder named after the archive in the working directory. Members are written as `{hash}.bntx`, `{hash}.csb`, or extension-less when the payload carries no recognizable magic.

Takes as many archives as you care to name, and accepts `--file` (`-f`), repeatable, and `--dir` (`-d`) `<DIR> <LEVEL>` the same way the other commands do. Walking a directory picks up `.arc` files only; a file you name yourself is always tried, whatever it is called. One bad archive is reported and skipped rather than ending the run.

`--output` (`-o`) is the destination folder for a single archive. Given several, it becomes the parent they each get a folder inside, so nothing overwrites anything else.

### DECODE
`neko-arc decode <FILE | DIR>`

Turns original names into hashes and, optionally, renames the unpacked files to match. Both modes work on the same targets, and a bare input is read for what it is: a directory behaves as `--dir`, anything else as `--file`.

By default every target is hashed and printed. A name does not have to exist on disk, so `neko-arc decode unit001.png` just works; a file that does exist is hashed by its file name alone, whatever folder it sits in.

- `--file` (`-f`) `<FILE>` works with one name, repeatable.
- `--dir` (`-d`) `<DIR> <LEVEL>` hashes every file name inside a directory, walking `LEVEL` deep (default `10`).
- `--dictionary` (`-c`) `<FILE>` reads a prebuilt `{hash},{name}` list. Printing lists every entry it holds alongside the targets.

`--rename` (`-r`) turns the targets around: they become the hashed files to rename, and the names come from the dictionary instead. `--file` renames that one file, `--dir` renames everything it walks into, and with no target at all the working directory is walked `10` deep. Without `--dictionary`, a `dictionary.csv` sitting beside the `neko-arc` binary is picked up on its own, so a dictionary kept there renames any tree you aim it at; with neither, there is nothing to rename from and the run stops. Add `--print` (`-p`) to list every rename as it happens.

Renaming keeps the container extension (`{hash}.bntx` becomes `{name}.png.bntx`). It runs just as well after `convert`: the extension `convert` guessed for a hashed table gives way to the real one, so `{hash}.csv` becomes `uni.imgcut` rather than `uni.imgcut.csv`, and a table whose guessed separator differs from the one its name calls for has its separators swapped on the way. Either order leaves you with the same files.

### DICTIONARY
`neko-arc dictionary <DIR> <LEVEL>`

Writes a `{hash},{name}` pair per line for every file found under `DIR`, walking `LEVEL` deep (default `10`). Lands in `dictionary.csv` in the working directory unless `--output` (`-o`) points elsewhere.

### CONVERT
`neko-arc convert <FILE | DIR>`

Converts `BNTX` to `PNG` and `CSB` to `CSV` / `TSV`, replacing the originals in place. Accepts `--file` (`-f`) and `--dir` (`-d`) `<DIR> <LEVEL>` the same way `decode` does. Pass `--output` (`-o`) to dump the results into a directory instead and leave the originals alone.

Reads block-linear and linear Tegra tiling, every `BCn` and `ASTC` block format, and the packed color formats, then applies the texture's own channel selectors.

Tables keep whatever name `decode` resolved for them, so `uni.imgcut.csb` becomes `uni.imgcut` and `stage.tsv.csb` becomes a tab-separated `stage.tsv`. Only `.tsv` uses tabs; every other table is comma-separated, which covers `.csv` alongside the `.imgcut`, `.mamodel`, and `.maanim` rigging formats. A file still under its hash has no name to go by, so the delimiter is picked from its contents and the matching extension is appended, which `decode --rename` corrects later once the name is known.

### FIX
`neko-arc fix <FILE | DIR>`

The lossy counterpart to `convert`. Everything here trades exact fidelity for data you can actually load, so reach for it only once you have decided you want that. Takes `--file` (`-f`) or `--dir` (`-d`) `<DIR> <LEVEL>` and repairs in place.

Each kind is opted into separately, because two of them consume their input:

- `--png` un-premultiplies alpha on `PNG` files, clearing the gray fringing that `BNTX` conversion leaves behind. Every game texture is premultiplied, so this applies to every semi-transparent pixel it finds; images made up entirely of fully opaque or fully transparent pixels come out untouched. This is what runs when no kind is chosen.
- `--bntx` decodes a container straight to a corrected image in one step, replacing the `.bntx`. Same result as `convert` followed by `--png`, without ever writing the premultiplied version to disk.
- `--csb` writes tables out the way `convert` does, except any field holding a line break or the table's own separator has those characters flattened to spaces, replacing the `.csb`. Without it a single field can silently split one row into several, or invent columns that were never there. Only the internal design spreadsheets that shipped with multi-line header cells need this.
- `--all` does every kind in one pass.

Un-premultiplying is not reversible, and an already-corrected image is indistinguishable from one that still needs it, so `--png` is a one-shot pass: run it over the same images twice and it will brighten them twice. Use `--bntx` on a freshly extracted tree when you want that decision taken out of your hands, since decoding and correcting in one step cannot double-apply.
