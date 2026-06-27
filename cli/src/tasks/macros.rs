/// Implement [`Task::dependencies`](crate::tasks::Task::dependencies) by expanding to the required
/// `fn dependencies(&self) -> &[TaskId]` method body.
///
/// The `const DEPS` intermediate is required because [`std::any::TypeId::of`]
/// is a `const fn` — placing it in a `const` ensures the slice has a
/// `'static` lifetime as required by the return type.  Each type is
/// wrapped in [`TaskId::Type`](crate::tasks::TaskId::Type) automatically.
///
/// # Examples
///
/// ```ignore
/// task_deps![super::reload_config::ReloadConfig, super::symlinks::InstallSymlinks]
/// ```
macro_rules! task_deps {
    [$($dep:ty),+ $(,)?] => {
        fn dependencies(&self) -> &[$crate::tasks::TaskId] {
            const DEPS: &[$crate::tasks::TaskId] = &[
                $($crate::tasks::TaskId::Type(std::any::TypeId::of::<$dep>())),+
            ];
            DEPS
        }
    };
}

pub(crate) use task_deps;

/// Implement [`Task::execution_policies`](crate::tasks::Task::execution_policies)
/// from a static policy list.
macro_rules! execution_policies_impl {
    [$($policy:expr),+ $(,)?] => {
        fn execution_policies(&self) -> &[$crate::tasks::ExecutionPolicy] {
            const POLICIES: &[$crate::tasks::ExecutionPolicy] = &[$($policy),+];
            POLICIES
        }
    };
}

pub(crate) use execution_policies_impl;

/// Process config-derived resources after a macro has evaluated its item list.
///
/// Keeping this logic in a normal function makes the macro expansion smaller
/// and keeps resource processing semantics in one place.
pub(crate) fn process_config_resources<Item, R>(
    ctx: &crate::tasks::Context,
    items: Vec<Item>,
    mut build: impl FnMut(Item, &crate::tasks::Context) -> R,
    opts: &crate::tasks::ProcessOpts,
) -> ::anyhow::Result<crate::tasks::TaskResult>
where
    R: crate::resources::IntrinsicState + Send,
{
    let resources = items.into_iter().map(|item| build(item, ctx));
    crate::tasks::process_resources(ctx, resources, opts)
}

/// Process config-derived resources whose state is supplied by one cache.
pub(crate) fn process_config_resources_with_provider<Item, Cache, R>(
    ctx: &crate::tasks::Context,
    items: Vec<Item>,
    mut build: impl FnMut(Item, &crate::tasks::Context) -> R,
    load: impl Fn(&[R], &crate::tasks::Context) -> ::anyhow::Result<Cache> + Sync,
    state: impl for<'a> Fn(&'a R, &Cache) -> ::anyhow::Result<crate::resources::ResourceState> + Sync,
    opts: &crate::tasks::ProcessOpts,
) -> ::anyhow::Result<crate::tasks::TaskResult>
where
    R: crate::resources::Resource + Send,
    Cache: Sync,
{
    let resources: Vec<R> = items.into_iter().map(|item| build(item, ctx)).collect();
    let cache = load(&resources, ctx)?;
    let provider = crate::resources::PreloadedStateProvider::new(cache, state);
    crate::tasks::process_resources_with_provider(ctx, resources, &provider, opts)
}

/// Define a task that processes config-derived resources with minimal
/// boilerplate.
///
/// Generates a `Debug` struct and a full [`Task`](crate::tasks::Task) implementation for the
/// common pattern: read config items → build resources → process.
///
/// Two variants are supported:
///
/// - **Standard:** each resource computes its own state via
///   [`IntrinsicState::current_state`](crate::resources::IntrinsicState::current_state).
///   Required fields: `items`, `build`, `opts`. Optional: `deps`, `guard`,
///   `setup`.
/// - **Batch:** resources are built once, then a state provider loads one
///   cache for the full set and maps each resource to a state. Use when state
///   checking amortises across the set (e.g. registry, VS Code extensions).
///   Required fields: `items`, `cache`,
///   `build`, `state`, `opts`. Optional: `deps`, `guard`. The arm is
///   selected by the presence of `cache:` and `state:`.
///
/// # Examples
///
/// ```ignore
/// // Standard variant
/// resource_task! {
///     pub StructName {
///         name: "Human-readable task name",
///         phase: TaskPhase::Provision,
///         domain: Domain::Packages,
///         policy: [PlatformCapability::Systemd.policy()], // optional
///         deps: [DepType1, DepType2],          // optional
///         guard: |ctx| bool_expr,              // optional
///         setup: |ctx| { side_effects(); },    // optional
///         items: |ctx| ctx.config_read().field.clone(),
///         build: |item, ctx| Resource::from(&item, &ctx.home),
///         opts: ProcessOpts::strict("verb"),
///     }
/// }
///
/// // Batch variant
/// resource_task! {
///     pub StructName {
///         name: "Human-readable task name",
///         phase: TaskPhase::Provision,
///         domain: Domain::Packages,
///         items: |ctx| ctx.config_read().field.clone(),
///         cache: |resources, ctx| query_bulk_state(resources, ctx),
///         build: |item, ctx| Resource::from(&item, &ctx.home),
///         state: |resource, cache| resource.state_from_cache(&cache),
///         opts: ProcessOpts::lenient("verb"),
///     }
/// }
/// ```
macro_rules! resource_task {
    // -----------------------------------------------------------------
    // Batch variant — `cache:` and `state:` blocks are present.
    // -----------------------------------------------------------------
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            name: $task_name:expr,
            phase: $phase:expr,
            domain: $domain:expr,
            $(policy: [$($policy:expr),+ $(,)?],)?
            $(deps: [$($dep:ty),+ $(,)?],)?
            $(guard: |$guard_ctx:ident| $guard_expr:expr,)?
            items: |$items_ctx:ident| $items_expr:expr,
            cache: |$cache_items:ident, $cache_ctx:ident| $cache_expr:expr,
            build: |$item:ident, $build_ctx:ident| $build_expr:expr,
            state: |$state_res:ident, $state_cache:ident| $state_expr:expr,
            opts: $opts:expr $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug)]
        $vis struct $name;

        impl $name {
            #[allow(clippy::shadow_unrelated, reason = "macro hygiene")]
            fn run_batch(
                ctx: &$crate::tasks::Context,
            ) -> ::anyhow::Result<$crate::tasks::TaskResult> {
                let $items_ctx = ctx;
                let items: Vec<_> = { $items_expr };
                if items.is_empty() {
                    return Ok($crate::tasks::TaskResult::NotApplicable(
                        "nothing configured".to_string(),
                    ));
                }
                ctx.debug_fmt(|| {
                    format!(
                        "batch-checking {} resources with a single query",
                        items.len()
                    )
                });
                $crate::tasks::process_config_resources_with_provider(
                    ctx,
                    items,
                    |$item, $build_ctx| $build_expr,
                    |$cache_items, $cache_ctx| $cache_expr,
                    |$state_res, $state_cache| Ok($state_expr),
                    &$opts,
                )
            }
        }

        impl $crate::tasks::Task for $name {
            fn name(&self) -> &'static str {
                $task_name
            }

            fn phase(&self) -> $crate::tasks::TaskPhase {
                $phase
            }

            fn domain(&self) -> $crate::tasks::Domain {
                $domain
            }

            $($crate::tasks::task_deps![$($dep),+];)?

            $($crate::tasks::execution_policies_impl![$($policy),+];)?

            #[allow(
                unused_variables,
                reason = "ctx is only used when the generated task declares a guard"
            )]
            fn should_run(&self, ctx: &$crate::tasks::Context) -> bool {
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return false; }
                )?
                true
            }

            fn run_if_applicable(
                &self,
                ctx: &$crate::tasks::Context,
            ) -> ::anyhow::Result<Option<$crate::tasks::TaskResult>> {
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return Ok(None); }
                )?
                ctx.log.stage($task_name);
                let result = Self::run_batch(ctx)?;
                if matches!(result, $crate::tasks::TaskResult::NotApplicable(_)) {
                    return Ok(None);
                }
                Ok(Some(result))
            }

            fn run(
                &self,
                ctx: &$crate::tasks::Context,
            ) -> ::anyhow::Result<$crate::tasks::TaskResult> {
                Self::run_batch(ctx)
            }
        }
    };

    // -----------------------------------------------------------------
    // Standard variant — each resource computes its own state.
    // -----------------------------------------------------------------
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            name: $task_name:expr,
            phase: $phase:expr,
            domain: $domain:expr,
            $(policy: [$($policy:expr),+ $(,)?],)?
            $(deps: [$($dep:ty),+ $(,)?],)?
            $(guard: |$guard_ctx:ident| $guard_expr:expr,)?
            $(setup: |$setup_ctx:ident| $setup_expr:expr,)?
            items: |$items_ctx:ident| $items_expr:expr,
            build: |$item:ident, $build_ctx:ident| $build_expr:expr,
            opts: $opts:expr $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug)]
        $vis struct $name;

        impl $crate::tasks::Task for $name {
            fn name(&self) -> &'static str {
                $task_name
            }

            fn phase(&self) -> $crate::tasks::TaskPhase {
                $phase
            }

            fn domain(&self) -> $crate::tasks::Domain {
                $domain
            }

            $($crate::tasks::task_deps![$($dep),+];)?

            $($crate::tasks::execution_policies_impl![$($policy),+];)?

            #[allow(
                unused_variables,
                reason = "ctx is only used when the generated task declares a guard"
            )]
            fn should_run(&self, ctx: &$crate::tasks::Context) -> bool {
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return false; }
                )?
                true
            }

            fn run_if_applicable(
                &self,
                ctx: &$crate::tasks::Context,
            ) -> ::anyhow::Result<Option<$crate::tasks::TaskResult>> {
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return Ok(None); }
                )?
                let $items_ctx = ctx;
                let items: Vec<_> = { $items_expr };
                if items.is_empty() {
                    return Ok(None);
                }
                ctx.log.stage($task_name);
                $(
                    let $setup_ctx = ctx;
                    { $setup_expr }
                )?
                $crate::tasks::process_config_resources(
                    ctx,
                    items,
                    |$item, $build_ctx| $build_expr,
                    &$opts,
                )
                .map(Some)
            }

            fn run(
                &self,
                ctx: &$crate::tasks::Context,
            ) -> ::anyhow::Result<$crate::tasks::TaskResult> {
                let $items_ctx = ctx;
                let items: Vec<_> = { $items_expr };
                if items.is_empty() {
                    return Ok($crate::tasks::TaskResult::NotApplicable(
                        "nothing configured".to_string(),
                    ));
                }
                $(
                    let $setup_ctx = ctx;
                    { $setup_expr }
                )?
                $crate::tasks::process_config_resources(
                    ctx,
                    items,
                    |$item, $build_ctx| $build_expr,
                    &$opts,
                )
            }
        }
    };
}

pub(crate) use resource_task;
