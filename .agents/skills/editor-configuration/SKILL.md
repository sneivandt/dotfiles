---
name: editor-configuration
description: >
  Use for shared Vim/Neovim configuration under symlinks/vim/, lazy.nvim plugin
  declarations, bootstrap pins, lockfiles, mappings, or editor startup tests.
  Not for VS Code extension resources or generic Rust editor tooling.
---

# Editor Configuration

Read the [editor guide](../../../symlinks/vim/README.md) for entry points,
bootstrap, parser installation, plugin maintenance, and troubleshooting.

## Choose the layer

| Change | Owner under `symlinks/vim/` |
|---|---|
| shared editing behavior | `vimrc` |
| Neovim entry point | `init.vim` |
| Neovim terminal behavior and mappings | `nvimrc` |
| plugins and plugin configuration | `lua/lazy-bootstrap.lua` |
| recorded plugin revisions | `lazy-lock.json` |

Preserve plain Vim startup without third-party plugins. Keep filetype/window
options local where intended, and test transitions between buffers or windows,
not just the initial value. Preserve terminal input and focus behavior when
changing Escape or navigation mappings.

## Keep plugin changes deliberate

- Ordinary Neovim startup can clone lazy.nvim and install missing plugins;
  launching the installed editor is not a read-only check.
- Review lockfile changes as dependency changes. A bootstrap-pin change must
  update its matching lockfile entry without advancing unrelated plugins.
- Exercise a bootstrap change with an absent manager in isolated data paths;
  an existing installation does not prove first-launch behavior.
- Keep parser installation explicit as documented. Do not make ordinary startup
  install parsers or block on downloads to hide a missing-parser condition.
- Diagnose the affected plugin/revision before replacing installed content;
  do not delete the whole plugin tree as a general repair.

## Verify the affected behavior

Start with the [plugin-free tests](../../../symlinks/vim/tests/), which use
isolated homes and stub plugin loading. Use
[Application configuration](../../../docs/TESTING.md#application-configuration)
for commands and the separate CI plugin-restore checks. Headless/stubbed tests
prove their assertions, not interactive rendering or real plugin integration.
Keep the tracked lockfile unchanged during verification unless updating its
revisions is part of the requested change. Live plugin updates and cleanup
require authorization for that action.
