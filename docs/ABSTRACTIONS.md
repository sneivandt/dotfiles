# Abstractions Guide

This guide explains the new abstraction layers introduced to improve code quality, maintainability, and extensibility.

## Table of Contents

- [Configuration Validation Layer](#configuration-validation-layer)
- [Unified Resource Manager](#unified-resource-manager)
- [Benefits](#benefits)
- [Future Extensions](#future-extensions)

---

## Configuration Validation Layer

The **Configuration Validation Layer** provides early detection of configuration issues during load time rather than runtime.

### Architecture

Located in `cli/src/config/validation.rs`, the layer consists of:

1. **`ConfigValidator` trait** - Common interface for all validators
2. **Validator implementations** - One per configuration type (symlinks, packages, registry, etc.)
3. **`ValidationWarning` struct** - Structured warning messages
4. **`validate_all()` function** - Coordinates all validators

### How It Works

When configuration is loaded via `Config::load()`, validators automatically check for:

- **Missing files** - Symlink sources that don't exist
- **Invalid values** - Malformed chmod modes, invalid registry hives
- **Platform mismatches** - AUR packages on non-Arch systems, registry entries on Linux
- **Format errors** - VS Code extension IDs without publisher, systemd units without proper extensions

Warnings are displayed during the "Loading configuration" stage:

```
⚙ Loading configuration
⚠ found 2 configuration warning(s):
⚠   symlinks.ini [config/nonexistent]: source file does not exist: /home/user/dotfiles/symlinks/config/nonexistent
⚠   packages.ini [yay]: AUR package specified but platform is not Arch Linux
```

### Adding a New Validator

To add validation for a new configuration type:

1. **Define the validator struct:**

```rust
pub struct MyConfigValidator {
    items: Vec<super::my_config::MyItem>,
}

impl MyConfigValidator {
    #[must_use]
    pub const fn new(items: Vec<super::my_config::MyItem>) -> Self {
        Self { items }
    }
}
```

2. **Implement the `ConfigValidator` trait:**

```rust
impl ConfigValidator for MyConfigValidator {
    fn validate(&self, root: &Path, platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();
        
        for item in &self.items {
            // Check for issues
            if item.has_problem() {
                warnings.push(ValidationWarning::new(
                    "my-config.ini",
                    &item.name,
                    "description of the problem",
                ));
            }
        }
        
        warnings
    }
    
    #[allow(dead_code)]
    fn name(&self) -> &'static str {
        "my-config"
    }
}
```

3. **Register in `validate_all()`:**

```rust
pub fn validate_all(config: &super::Config, platform: &Platform) -> Vec<ValidationWarning> {
    let validators: Vec<Box<dyn ConfigValidator>> = vec![
        // ... existing validators
        Box::new(MyConfigValidator::new(config.my_items.clone())),
    ];
    
    // ... rest of function
}
```

### Testing

All validators include comprehensive tests:

```rust
#[test]
fn my_validator_detects_invalid_value() {
    let items = vec![
        super::super::my_config::MyItem {
            value: "invalid".to_string(),
        },
    ];
    
    let validator = MyConfigValidator::new(items);
    let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Linux, false));
    
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("invalid"));
}
```

---

## Unified Resource Manager

The **Unified Resource Manager** provides a consistent interface for all declarative resources (symlinks, registry entries, file permissions, etc.).

### Architecture

Located in `cli/src/resources/`, the system consists of:

1. **`Resource` trait** - Common interface for all resources
2. **`ResourceState` enum** - Current state of a resource (Missing, Correct, Incorrect, Invalid)
3. **`ResourceChange` enum** - Result of applying a change (Applied, AlreadyCorrect, Skipped)
4. **Resource implementations** - One per resource type (currently: `SymlinkResource`)

### Resource Lifecycle

Every resource follows this lifecycle:

```
1. Check current state    → current_state() → ResourceState
2. Determine if needed    → needs_change() → bool
3. Apply if needed        → apply() → ResourceChange
```

### Using the Resource Abstraction

The Resource trait has been implemented for three resource types:
- **SymlinkResource** - File system symbolic links
- **RegistryResource** - Windows registry entries  
- **ChmodResource** - Unix file permissions

#### Example: Working with a Symlink Resource

```rust
use crate::resources::{Resource, ResourceState, ResourceChange};
use crate::resources::symlink::SymlinkResource;

// Create a symlink resource
let resource = SymlinkResource::new(
    source_path,  // PathBuf: what the symlink points to
    target_path,  // PathBuf: where the symlink will be created
);

// Check current state
match resource.current_state()? {
    ResourceState::Missing => println!("Symlink doesn't exist"),
    ResourceState::Correct => println!("Symlink is already correct"),
    ResourceState::Incorrect { current } => println!("Symlink is wrong: {}", current),
    ResourceState::Invalid { reason } => println!("Can't create symlink: {}", reason),
}

// Check if change is needed
if resource.needs_change()? {
    println!("Change needed for: {}", resource.description());
    
    // Apply the change (creates parent dirs, removes old symlink, creates new one)
    match resource.apply()? {
        ResourceChange::Applied => println!("Successfully applied"),
        ResourceChange::AlreadyCorrect => println!("Already correct"),
        ResourceChange::Skipped { reason } => println!("Skipped: {}", reason),
    }
}
```

#### Example: Working with a Registry Resource (Windows)

```rust
use crate::resources::registry::RegistryResource;

// Create from config entry
let entry = &ctx.config.registry[0];
let resource = RegistryResource::from_entry(entry);

// Or create directly
let resource = RegistryResource::new(
    "HKCU:\\Console".to_string(),
    "FontSize".to_string(),
    "14".to_string(),
);

// Check and apply
if resource.needs_change()? {
    resource.apply()?;
}
```

#### Example: Working with a Chmod Resource (Unix)

```rust
use crate::resources::chmod::ChmodResource;

// Create from config entry
let entry = &ctx.config.chmod[0];
let resource = ChmodResource::from_entry(entry, &ctx.home);

// Or create directly
let resource = ChmodResource::new(
    PathBuf::from("/home/user/.ssh/config"),
    "600".to_string(),
);

// Check and apply
match resource.current_state()? {
    ResourceState::Correct => println!("Permissions already correct"),
    ResourceState::Incorrect { current } => {
        println!("Current: {}, applying change...", current);
        resource.apply()?;
    }
    ResourceState::Invalid { reason } => println!("Cannot apply: {}", reason),
    _ => {}
}
```

#### Task Integration Pattern

Tasks can use resources to simplify their logic:

```rust
impl Task for MyTask {
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut stats = TaskStats::new();
        
        for config_item in &ctx.config.my_items {
            let resource = MyResource::new(config_item);
            
            // Check state
            let state = resource.current_state()?;
            match state {
                ResourceState::Invalid { reason } => {
                    ctx.log.debug(&format!("skipping: {}", reason));
                    stats.skipped += 1;
                    continue;
                }
                ResourceState::Correct => {
                    ctx.log.debug(&format!("ok: {}", resource.description()));
                    stats.already_ok += 1;
                    continue;
                }
                _ => {}
            }
            
            // Dry-run or apply
            if ctx.dry_run {
                ctx.log.dry_run(&format!("would apply: {}", resource.description()));
                stats.changed += 1;
            } else {
                match resource.apply()? {
                    ResourceChange::Applied => {
                        ctx.log.debug(&format!("applied: {}", resource.description()));
                        stats.changed += 1;
                    }
                    ResourceChange::Skipped { reason } => {
                        ctx.log.debug(&format!("skipped: {}", reason));
                        stats.skipped += 1;
                    }
                    _ => {}
                }
            }
        }
        
        Ok(stats.finish(ctx))
    }
}
```

### Creating a New Resource Type

To add a new resource type (e.g., for registry entries or chmod):

1. **Define the resource struct:**

```rust
#[derive(Debug, Clone)]
pub struct MyResource {
    pub config_data: MyConfigData,
}

impl MyResource {
    #[must_use]
    pub const fn new(config_data: MyConfigData) -> Self {
        Self { config_data }
    }
}
```

2. **Implement the `Resource` trait:**

```rust
impl Resource for MyResource {
    fn description(&self) -> String {
        format!("my resource: {}", self.config_data.name)
    }
    
    fn current_state(&self) -> Result<ResourceState> {
        // Check if resource is missing, correct, incorrect, or invalid
        // Return appropriate ResourceState
        
        if !self.can_be_applied() {
            return Ok(ResourceState::Invalid {
                reason: "reason why resource cannot be applied".to_string(),
            });
        }
        
        if self.is_missing() {
            return Ok(ResourceState::Missing);
        }
        
        if self.is_correct() {
            return Ok(ResourceState::Correct);
        }
        
        Ok(ResourceState::Incorrect {
            current: "description of current state".to_string(),
        })
    }
    
    fn apply(&self) -> Result<ResourceChange> {
        // Apply the change to make the resource match the desired state
        // This is only called when NOT in dry-run mode
        
        // Create any necessary parent directories
        // Remove any existing conflicting resources
        // Create/update the resource
        
        Ok(ResourceChange::Applied)
    }
}
```

3. **Add tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn resource_detects_missing_state() {
        let resource = MyResource::new(test_data());
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }
    
    #[test]
    fn resource_applies_successfully() {
        let resource = MyResource::new(test_data());
        let result = resource.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);
    }
}
```

### Resource State Machine

Resources follow this state machine:

```
        ┌─────────────────────────────────────────┐
        │                                         │
        │  Initial Check: current_state()         │
        │                                         │
        └──────────┬──────────────────────────────┘
                   │
                   ├─→ Missing ──────────┐
                   │                     │
                   ├─→ Incorrect ────────┼─→ needs_change() = true
                   │                     │
                   ├─→ Correct ──────────┼─→ needs_change() = false
                   │                     │
                   └─→ Invalid ──────────┘
                                         │
                                         │ (if needs_change)
                                         ▼
                                   apply() called
                                         │
                                         ├─→ Applied
                                         ├─→ AlreadyCorrect
                                         └─→ Skipped { reason }
```

---

## Benefits

### 1. Early Error Detection

Configuration errors are caught during loading rather than during task execution:

**Before:**
```
✓ Install symlinks
  → Error: source file does not exist
```

**After:**
```
⚠ found 1 configuration warning(s):
⚠   symlinks.ini [nonexistent]: source file does not exist
✓ Install symlinks (skipped 1, 5 changed)
```

### 2. Consistent Resource Handling

All resources share the same state checking and application pattern:

- **State checking** is consistent across symlinks, registry, chmod
- **Error handling** is centralized
- **Dry-run support** is built into the abstraction
- **Statistics tracking** follows the same pattern

### 3. Improved Testability

Both abstractions enable easier unit testing:

- **Validators** can be tested independently of the full config loading
- **Resources** can be tested in isolation without filesystem operations
- **Mocking** is easier with well-defined interfaces

### 4. Better Separation of Concerns

- **Configuration layer** handles parsing and validation
- **Resource layer** handles state and operations
- **Task layer** handles orchestration and logging

### 5. Extensibility

Adding new config types or resource types requires minimal changes:

- Implement the trait
- Add tests
- Register in the appropriate collection

---

## Future Extensions

### Planned Enhancements

The abstraction layer enables several future improvements:

#### 1. Parallel Resource Application

Resources with no dependencies could be applied in parallel:

```rust
// Future: parallel execution for independent resources
let results: Vec<Result<ResourceChange>> = resources
    .par_iter()  // Rayon parallel iterator
    .map(|r| r.apply())
    .collect();
```

#### 2. Transaction/Rollback Support

Resources could support undo operations:

```rust
trait ReversibleResource: Resource {
    fn checkpoint(&self) -> Result<ResourceCheckpoint>;
    fn rollback(&self, checkpoint: ResourceCheckpoint) -> Result<()>;
}
```

#### 3. Dependency-Aware Validation

Validators could check cross-configuration dependencies:

```rust
impl ConfigValidator for SymlinkValidator {
    fn validate_with_context(&self, config: &Config) -> Vec<ValidationWarning> {
        // Check if symlinked files have matching chmod entries
        // Check if symlinks reference files that exist in the profile
    }
}
```

#### 4. Resource Diff Preview

The abstraction enables a "diff" command showing exactly what would change:

```bash
$ dotfiles diff
Configuration changes:
  symlinks:
    + .bashrc -> /home/user/dotfiles/symlinks/bashrc
    ~ .gitconfig (currently points to /old/path)
  registry:
    + HKCU:\Console\FontSize = 14
  chmod:
    ~ .ssh/config (currently 644, will change to 600)
```

#### 5. Platform-Specific Resource Implementations

Resources could have platform-specific variants:

```rust
enum PlatformSymlink {
    Unix(UnixSymlink),
    Windows(WindowsSymlink),
}

impl From<&Platform> for PlatformSymlink {
    fn from(platform: &Platform) -> Self {
        match platform.os {
            Os::Linux => Self::Unix(UnixSymlink::new()),
            Os::Windows => Self::Windows(WindowsSymlink::new()),
        }
    }
}
```

---

## Related Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) - Overall system design
- [CUSTOMIZATION.md](CUSTOMIZATION.md) - Adding new configuration
- [CONTRIBUTING.md](CONTRIBUTING.md) - Development guidelines

---

## Questions or Issues?

If you encounter issues with the new abstractions or have suggestions for improvements:

1. Check existing tests in `cli/src/config/validation.rs` and `cli/src/resources/`
2. Review the trait documentation in the source files
3. Open an issue on GitHub with details about your use case
