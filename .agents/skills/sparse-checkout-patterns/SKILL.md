---
name: sparse-checkout-patterns
description: >
  Sparse-checkout manifest conventions for this dotfiles repo. Use when changing
  conf/manifest.toml, category-based file exclusions, profile-driven checkout
  filtering, or sparse-checkout application.
---

# Sparse Checkout Patterns

The sparse checkout system allows a single repo to support multiple environments
by controlling which files are checked out based on the selected profile.

**Key files:**
- `conf/manifest.toml` — maps `symlinks/` paths to exclusion categories
- `conf/profiles.toml` — defines which categories each profile excludes
- `cli/src/domains/repository/sparse_checkout.rs` — task entry point
- `cli/src/domains/repository/sparse_checkout/` — supporting implementation and tests

## How It Works

1. Profile resolves `excluded_categories` via `conf/profiles.toml`
2. OS auto-detection adds platform overrides (Linux always excludes `windows`; non-Arch always excludes `arch`)
3. `get_excluded_files()` loads `manifest.toml` against `excluded_categories`
4. Sparse checkout patterns are generated (`/*` + `!/<path>` entries) and written to `.git/info/sparse-checkout`
5. `git read-tree -mu HEAD` updates the working directory

## Manifest File Format

Paths are relative to `symlinks/`. Section names use the same AND logic as
other config files and describe the active category combinations that own each
path:

```toml
[windows]
paths = ["AppData/", "config/git/windows"]   # excluded if windows is excluded

[arch]
paths = ["config/pacman.conf", "config/paru/"]

[desktop]
paths = ["config/Code/", "config/rofi/"]

[arch-desktop]
paths = ["config/hypr/", "config/dunst/"]    # retained only when BOTH are active
```

- A path is retained when any section declaring it is active; otherwise it is excluded
- Directories **must** end with `/`
- Files in the `[base]` profile are never excluded — don't list them here

## Adding a New File

1. Determine which active category combination owns the file
2. Add to the appropriate section in `conf/manifest.toml` (relative to `symlinks/`)
3. Run `./dotfiles.sh install -p <profile> -d` to verify the exclusion
4. Apply sparse checkout only from a clean working tree

## Relationship with Config Processing

The AND-logic section name has the same ownership meaning everywhere:

| File | Filtered against | Meaning of `[arch-desktop]` |
|---|---|---|
| `manifest.toml` | `active_categories` | retain only if **both** arch and desktop are active |
| `packages.toml`, `symlinks.toml`, etc. | `active_categories` | include only if **both** arch and desktop are active |

When multiple sections declare the same path, any active owner keeps it in the
checkout. This supports genuinely shared paths without broadening their
category ownership.
