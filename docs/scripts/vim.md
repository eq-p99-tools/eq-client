# Vim and Neovim

[Help home](README.md)
[Language reference](reference.md)

The repository includes an MIT-licensed syntax plugin in `contrib/vim`. It recognizes `.eqscript` files and colors commands, numbers, key chords, and `#` comments. It supplies comment and indentation settings for this file type only. Highlighting is presentation, not validation; the client parser still checks whether an action is valid.

## Install

Copy the entire `contrib/vim` folder into your editor's package directory, naming the copied folder `eqscript`:

- Vim on Linux/macOS: `~/.vim/pack/eq-client/start/eqscript`
- Vim on Windows: `~/vimfiles/pack/eq-client/start/eqscript`
- Neovim on Linux/macOS: `~/.local/share/nvim/site/pack/eq-client/start/eqscript`
- Neovim on Windows: `~/AppData/Local/nvim-data/site/pack/eq-client/start/eqscript`

The `ftdetect`, `ftplugin`, and `syntax` directories should sit directly inside `eqscript`. Use a Vim version with native package support (Vim 8 or newer), or Neovim. Restart the editor after installing.

If you have disabled automatic file types or highlighting, enable them in your configuration:

```vim
filetype plugin on
syntax enable
```

## Use

Open a `.eqscript` file from your EQ installation's `Scripts` folder. For an existing `.txt` or `.script` file, use `:setfiletype eqscript`; the plugin deliberately does not take over those generic extensions. Use `:setlocal number` for line numbers.

Save externally, then use Reload in the client's workbench to see and run your changes. The client's save conflict check protects edits made in another editor. This plugin does not run scripts, connect to the game, or install a language server.
