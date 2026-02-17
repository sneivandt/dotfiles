# Customization Guide

Guide to customizing and extending the dotfiles system for your needs.

## Adding Configuration Files

### Adding a New Symlink

1. **Place file in symlinks directory**:
   ```bash
   # Create file structure (note: no leading dot in symlinks/)
   mkdir -p symlinks/config/mynewapp
   echo "my config" > symlinks/config/mynewapp/config.yml
   ```

2. **Add entry to conf/symlinks.ini**:
   ```ini
   [base]
   config/mynewapp/config.yml
   ```
   Or for profile-specific config:
   ```ini
   [arch,desktop]
   config/mynewapp/config.yml
   ```

3. **Optionally, categorize in manifest.ini** (if file should be excluded in certain profiles):
   ```ini
   [desktop]
   symlinks/config/mynewapp/
   ```

4. **Install symlink**:
   ```bash
   ./dotfiles.sh install
   ```

The file will be symlinked from `symlinks/config/mynewapp/config.yml` to `~/.config/mynewapp/config.yml`.

### Adding Multiple Related Files

For complex applications with multiple config files:

```bash
# Create directory structure
mkdir -p symlinks/config/myapp
touch symlinks/config/myapp/config.yml
touch symlinks/config/myapp/themes.yml
touch symlinks/config/myapp/plugins.yml
```

Add to `conf/symlinks.ini`:
```ini
[base]
config/myapp
```

This links the entire directory.

## Adding Packages

### Adding System Packages (Linux)

1. **Edit conf/packages.ini**:
   ```ini
   [arch]
   my-package
   another-package

   [arch,desktop]
   desktop-package
   ```

2. **Find correct package name**:
   ```bash
   # Official repositories
   pacman -Ss package-name

   # AUR
   paru -Ss package-name
   ```

3. **Add to appropriate section**:
   - `[arch]` - Available on all Arch systems
   - `[arch,desktop]` - Only for desktop systems
   - `[arch,aur]` - AUR packages (requires paru)
   - `[arch,desktop,aur]` - AUR packages for desktop only

4. **Install**:
   ```bash
   ./dotfiles.sh install
   ```

### Adding Windows Packages

1. **Find package ID**:
   ```powershell
   winget search <package-name>
   # Note the exact package ID (e.g., Microsoft.PowerShell)
   ```

2. **Edit conf/packages.ini**:
   ```ini
   [windows]
   Microsoft.PowerShell
   Microsoft.VisualStudioCode
   Git.Git
   ```

3. **Install**:
   ```powershell
   .\dotfiles.ps1 install -p windows
   ```

## Adding Systemd Units

### Adding a User Unit

1. **Create unit file in symlinks**:
   ```bash
   mkdir -p symlinks/config/systemd/user
   cat > symlinks/config/systemd/user/my-service.service << 'EOF'
   [Unit]
   Description=My Custom Service

   [Service]
   ExecStart=/usr/bin/myapp

   [Install]
   WantedBy=default.target
   EOF
   ```

2. **Add unit file to symlinks.ini**:
   ```ini
   [base]
   config/systemd/user/my-service.service
   ```

3. **Add to units.ini to enable it**:
   ```ini
   [base]
   my-service.service
   ```

4. **Install and enable**:
   ```bash
   ./dotfiles.sh install
   # Unit is automatically symlinked and enabled
   ```

### Adding a Timer Unit

Timer units require both a service and timer file:

```bash
# Create service file
cat > symlinks/config/systemd/user/my-task.service << 'EOF'
[Unit]
Description=My Periodic Task

[Service]
Type=oneshot
ExecStart=/usr/bin/my-script.sh
EOF

# Create timer file
cat > symlinks/config/systemd/user/my-task.timer << 'EOF'
[Unit]
Description=Run My Task Daily

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
EOF
```

Add both to `conf/symlinks.ini` and add the timer to `conf/units.ini`:
```ini
# symlinks.ini
[base]
config/systemd/user/my-task.service
config/systemd/user/my-task.timer

# units.ini
[base]
my-task.timer
```

## Adding VS Code Extensions

1. **Find extension ID**:
   - Open VS Code
   - Go to Extensions
   - Click extension
   - Copy ID (e.g., `ms-python.python`)

2. **Add to conf/vscode-extensions.ini**:
   ```ini
   [extensions]
   ms-python.python
   rust-lang.rust-analyzer
   github.copilot
   ```

3. **Install**:
   ```bash
   ./dotfiles.sh install
   ```

## Adding GitHub Copilot CLI Skills

GitHub Copilot CLI can be extended with custom skills that provide additional context and functionality.

1. **Find skill folder URL**:
   - Browse GitHub repositories with Copilot skills
   - Find the folder containing skill definition files
   - Copy the GitHub URL (e.g., `https://github.com/owner/repo/blob/main/skills/skill-name`)

2. **Add to conf/copilot-skills.ini**:
   ```ini
   [base]
   https://github.com/github/awesome-copilot/blob/main/skills/azure-devops-cli
   https://github.com/microsoft/skills/blob/main/.github/skills/azure-identity-dotnet

   [desktop]
   https://github.com/example/skills/blob/main/skills/web-dev-helper
   ```

3. **Install**:
   ```bash
   ./dotfiles.sh install
   ```
   Or on Windows:
   ```powershell
   .\dotfiles.ps1 install -p windows
   ```

**Notes**:
- Skills are downloaded to `~/.copilot/skills/` directory
- The entire folder (including subdirectories) is downloaded
- Requires GitHub Copilot CLI (`gh copilot`) to be functional
- Skills are profile-aware - use appropriate sections

## Adding Registry Settings (Windows)

1. **Edit conf/registry.ini**:
   ```ini
   [HKCU:\Software\MyApp]
   Setting1 = Value1
   Setting2 = Value2

   [HKCU:\Console]
   FaceName = Consolas
   FontSize = 0x00140000
   ```

2. **Format values**:
   - Strings: `KeyName = String Value`
   - Numbers: `KeyName = 123` (decimal) or `KeyName = 0x7B` (hex)
   - DWORD: Automatically converted by script

3. **Apply**:
   ```powershell
   .\dotfiles.ps1 install -p windows
   ```

## Adding File Permissions

To set specific permissions on symlinked files:

1. **Edit conf/chmod.ini**:
   ```ini
   [base]
   600 ssh/config
   700 ssh/id_ed25519
   755 bin/my-script.sh

   [desktop]
   755 config/xmonad/scripts/startup.sh
   ```

2. **Format**: `<mode> <path-relative-to-home>`
   - Path is relative to `$HOME`, without leading dot
   - Mode is standard Unix permission (e.g., 644, 755, 600)

3. **Apply**:
   ```bash
   ./dotfiles.sh install
   ```

## Adding Fonts

1. **Add font files to repository**:
   ```bash
   # Place font files in appropriate location
   # (They should be downloaded/installed, not committed)
   ```

2. **Edit conf/fonts.ini**:
   ```ini
   [fonts]
   Source Code Pro
   Noto Color Emoji
   JetBrains Mono
   ```

3. **Install**:
   ```bash
   ./dotfiles.sh install
   # Fonts are checked and installed if missing
   # Font cache is automatically updated
   ```

## Creating Custom Profiles

### Simple Custom Profile

1. **Define profile in conf/profiles.ini**:
   ```ini
   [my-server]
   include=arch
   exclude=windows,desktop
   ```

2. **Add profile-specific packages**:
   ```ini
   # In packages.ini
   [my-server]
   docker
   nginx
   postgresql
   ```

3. **Add profile-specific symlinks**:
   ```ini
   # In symlinks.ini
   [my-server]
   config/docker/daemon.json
   config/nginx/nginx.conf
   ```

4. **Use profile**:
   ```bash
   ./dotfiles.sh install -p my-server
   ```

### Complex Profile with Dependencies

For a profile that needs specific combinations:

```ini
# profiles.ini
[web-dev]
include=desktop
exclude=windows,arch
```

Use multi-category sections for fine-grained control:
```ini
# packages.ini
[desktop]
code
nodejs
npm

# symlinks.ini
[desktop]
config/Code/User/settings.json
config/npm/npmrc
```

## Creating New Categories

Categories allow grouping configuration for specific use cases.

1. **Define category in profiles**:
   ```ini
   # profiles.ini
   [my-profile]
   include=my-category
   exclude=windows
   ```

2. **Use in configuration sections**:
   ```ini
   # packages.ini
   [my-category]
   package-one
   package-two

   [arch,my-category]
   arch-specific-package
   ```

3. **Map files in manifest.ini**:
   ```ini
   [my-category]
   symlinks/config/my-category-app/
   ```

## Advanced Customization

### Conditional Configuration

Use multi-category sections for conditional configuration:

```ini
# Only installed when BOTH arch AND desktop are active
[arch,desktop]
xorg-server
xmonad
alacritty
```

### Excluding Platform-Specific Files

Add to `manifest.ini` to exclude files from sparse checkout:

```ini
[windows]
symlinks/config/windows-specific/

[arch]
symlinks/config/arch-specific/

[desktop]
symlinks/config/xmonad/
symlinks/xinitrc
```

### Organizing Large Configurations

For large configuration sections, consider:

1. **Logical grouping**:
   ```ini
   [arch]
   # Core tools
   git
   base-devel
   vim

   # Development
   go
   rust
   python
   ```

2. **Profile-specific sections**:
   ```ini
   [arch]
   core-package

   [arch,desktop]
   desktop-package

   [arch,desktop,aur]
   aur-desktop-package
   ```

### Sharing Configuration Across Profiles

Use the `base` profile for shared configuration:

```ini
# symlinks.ini
[base]
bashrc
vimrc
gitconfig

[desktop]
config/Code/User/settings.json

[arch,desktop]
xinitrc
config/xmonad/
```

## Testing Custom Configuration

### Dry-Run Testing

Always test with dry-run first:

```bash
./dotfiles.sh install -p my-custom -d -v
```

Review the output for:
- Files that will be checked out
- Symlinks that will be created
- Packages that will be installed
- Units that will be enabled

### Incremental Testing

Test changes incrementally:

1. **Add one package**:
   ```bash
   ./dotfiles.sh install -d
   ```

2. **Verify it appears in dry-run output**

3. **Actually install**:
   ```bash
   ./dotfiles.sh install
   ```

4. **Verify it worked**:
   ```bash
   which package-name
   ```

### Validation Testing

Run static analysis on configuration changes:

```bash
./dotfiles.sh test
```

This checks:
- INI file syntax
- Section format
- Profile definitions
- File references

## Examples

### Example: Adding Neovim Configuration

```bash
# 1. Create config files
mkdir -p symlinks/config/nvim
echo "set number" > symlinks/config/nvim/init.vim

# 2. Add to symlinks.ini
cat >> conf/symlinks.ini << 'EOF'

[base]
config/nvim
EOF

# 3. Add neovim package (if not already present)
cat >> conf/packages.ini << 'EOF'

[arch]
neovim
EOF

# 4. Install
./dotfiles.sh install -p arch-desktop
```

### Example: Custom Development Profile

```bash
# 1. Define profile
cat >> conf/profiles.ini << 'EOF'

[dev]
include=arch,desktop
exclude=windows
EOF

# 2. Add development packages
cat >> conf/packages.ini << 'EOF'

[dev]
docker
kubectl
terraform
EOF

# 3. Add development tools symlinks
mkdir -p symlinks/config/dev-tools
cat >> conf/symlinks.ini << 'EOF'

[dev]
config/dev-tools
EOF

# 4. Use profile
./dotfiles.sh install -p dev
```

### Example: Adding Custom Scripts

```bash
# 1. Create script directory
mkdir -p symlinks/bin

# 2. Add script
cat > symlinks/bin/my-tool.sh << 'EOF'
#!/bin/sh
set -o errexit
set -o nounset
echo "My custom tool"
EOF

# 3. Add to symlinks
cat >> conf/symlinks.ini << 'EOF'

[base]
bin/my-tool.sh
EOF

# 4. Set executable permission
cat >> conf/chmod.ini << 'EOF'

[base]
755 bin/my-tool.sh
EOF

# 5. Install
./dotfiles.sh install
```

## Best Practices

1. **Start with base profile** for shared configuration
2. **Use appropriate sections** for profile-specific items
3. **Test with dry-run** before actual installation
4. **Keep sections organized** with comments
5. **Document custom profiles** in comments
6. **Validate configuration** with `./dotfiles.sh test`
7. **Version control** all configuration changes
8. **Review logs** after installation

## See Also

- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Profile System](PROFILES.md) - Understanding profiles
- [Usage Guide](USAGE.md) - Installation and usage
- [Architecture](ARCHITECTURE.md) - Implementation details
