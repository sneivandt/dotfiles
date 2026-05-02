---
name: sparse-checkout-patterns
description: >
  Patterns for the sparse checkout and manifest system, which is central to the profile-based file filtering.
  Use when working with file exclusion categories, sparse checkout configuration, or manifest management.
---

# Sparse Checkout Patterns

The sparse checkout system allows a single repo to support multiple environments
by controlling which files are checked out based on the selected profile.

**Key files:**
- `conf/manifest.toml` — maps `symlinks/` paths to exclusion categories
- `conf/profiles.toml` — defines which categories each profile excludes
- `cli/src/phases/repository/sparse_checkout.rs` — task implementation

## How It Works

1. Profile resolves `excluded_categories` via `conf/profiles.toml`
2. OS auto-detection adds platform overrides (Linux always excludes `windows`; non-Arch always excludes `arch`)
3. `get_excluded_files()` loads `manifest.toml` against `excluded_categories`
4. Sparse checkout patterns are generated (`/*` + `!/<path>` entries) and written to `.git/info/sparse-checkout`
5. `git read-tree -mu HEAD` updates the working directory

## Manifest File Format

Paths are relative to `symlinks/`. Section names use the same AND logic as
other config files, but are filtered against `excluded_categories` (not
`active_categories`):

```toml
[windows]
paths = ["AppData/", "config/git/windows"]   # excluded if windows is excluded

[arch]
paths = ["config/pacman.conf", "config/paru/"]

[desktop]
paths = ["config/Code/", "config/rofi/"]

[arch-desktop]
paths = ["config/hypr/", "config/dunst/"]    # excluded only if BOTH arch AND desktop excluded
```

- Directories **must** end with `/`
- Files in the `[base]` profile are never excluded — don't list them here

## Adding a New File

1. Determine which categories should **not** have the file
2. Add to the appropriate section in `conf/manifest.toml` (relative to `symlinks/`)
3. Run `./dotfiles.sh install -p <profile> -d` to verify the exclusion

## Relationship with Config Processing

The AND-logic section name means different things depending on context:

| File | Filtered against | Meaning of `[arch-desktop]` |
|---|---|---|
| `manifest.toml` | `excluded_categories` | exclude only if **both** arch and desktop are excluded |
| `packages.toml`, `symlinks.toml`, etc. | `active_categories` | include only if **both** arch and desktop are active |

This ensures files are available whenever their corresponding config items are active.

## Rules

1. Use the same AND logic as all other category-filtered TOML files
2. Trailing slash required for directories
3. Use hyphen-separated names for multi-category sections: `[arch-desktop]`
4. Paths are relative to `symlinks/` — no prefix in the manifest
5. Base files are never excluded — only list files that need conditional checkout
6. Clean working directory required before applying sparse checkout

## Related

- **`profile-system`** skill — profile selection and category mapping
- **`toml-configuration`** skill — TOML format and category AND logic
