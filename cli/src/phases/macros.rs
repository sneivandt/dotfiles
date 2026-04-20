/// Implement [`Task::dependencies`] by expanding to the required
/// `fn dependencies(&self) -> &[TypeId]` method body.
///
/// The `const DEPS` intermediate is required because [`std::any::TypeId::of`]
/// is a `const fn` — placing it in a `const` ensures the slice has a
/// `'static` lifetime as required by the return type.
///
/// # Examples
///
/// ```ignore
/// task_deps![super::reload_config::ReloadConfig, super::symlinks::InstallSymlinks]
/// ```
macro_rules! task_deps {
    [$($dep:ty),+ $(,)?] => {
        fn dependencies(&self) -> &[std::any::TypeId] {
            const DEPS: &[std::any::TypeId] = &[$(std::any::TypeId::of::<$dep>()),+];
            DEPS
        }
    };
}

pub(crate) use task_deps;

/// Define a task that processes config-derived resources with minimal
/// boilerplate.
///
/// Generates a `Debug` struct and a full [`Task`] implementation for the
/// common pattern: read config items → build resources → process.
///
/// Two variants are supported:
///
/// - **Standard:** each resource computes its own state via
///   [`Resource::current_state`](crate::resources::Resource::current_state).
///   Required fields: `items`, `build`, `opts`. Optional: `deps`, `guard`,
///   `setup`.
/// - **Batch:** state is computed once for the full set via a single bulk
///   query, then the resulting `(Resource, ResourceState)` pairs are
///   processed. Use when state checking amortises across the set (e.g.
///   registry, VS Code extensions). Required fields: `items`, `cache`,
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
///         phase: TaskPhase::Apply,
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
///         phase: TaskPhase::Apply,
///         items: |ctx| ctx.config_read().field.clone(),
///         cache: |items, ctx| query_bulk_state(items, ctx),
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
            #[allow(clippy::shadow_unrelated)]
            fn run_batch(
                ctx: &$crate::phases::Context,
            ) -> ::anyhow::Result<$crate::phases::TaskResult> {
                let $items_ctx = ctx;
                let $cache_items: Vec<_> = { $items_expr };
                if $cache_items.is_empty() {
                    return Ok($crate::phases::TaskResult::NotApplicable(
                        "nothing configured".to_string(),
                    ));
                }
                ctx.debug_fmt(|| {
                    format!(
                        "batch-checking {} resources with a single query",
                        $cache_items.len()
                    )
                });
                let $cache_ctx = ctx;
                let $state_cache = { $cache_expr }?;
                let resource_states = $cache_items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    let $state_res = { $build_expr };
                    let state = { $state_expr };
                    ($state_res, state)
                });
                $crate::phases::process_resource_states(ctx, resource_states, &$opts)
            }
        }

        impl $crate::phases::Task for $name {
            fn name(&self) -> &'static str {
                $task_name
            }

            fn phase(&self) -> $crate::phases::TaskPhase {
                $phase
            }

            $($crate::phases::task_deps![$($dep),+];)?

            fn should_run(&self, ctx: &$crate::phases::Context) -> bool {
                let _ = ctx;
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return false; }
                )?
                true
            }

            fn run_if_applicable(
                &self,
                ctx: &$crate::phases::Context,
            ) -> ::anyhow::Result<Option<$crate::phases::TaskResult>> {
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return Ok(None); }
                )?
                ctx.log.stage($task_name);
                let result = Self::run_batch(ctx)?;
                if matches!(result, $crate::phases::TaskResult::NotApplicable(_)) {
                    return Ok(None);
                }
                Ok(Some(result))
            }

            fn run(
                &self,
                ctx: &$crate::phases::Context,
            ) -> ::anyhow::Result<$crate::phases::TaskResult> {
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

        impl $crate::phases::Task for $name {
            fn name(&self) -> &'static str {
                $task_name
            }

            fn phase(&self) -> $crate::phases::TaskPhase {
                $phase
            }

            $($crate::phases::task_deps![$($dep),+];)?

            fn should_run(&self, ctx: &$crate::phases::Context) -> bool {
                let _ = ctx;
                $(
                    let $guard_ctx = ctx;
                    if !{ $guard_expr } { return false; }
                )?
                true
            }

            fn run_if_applicable(
                &self,
                ctx: &$crate::phases::Context,
            ) -> ::anyhow::Result<Option<$crate::phases::TaskResult>> {
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
                let resources = items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    $build_expr
                });
                $crate::phases::process_resources(ctx, resources, &$opts).map(Some)
            }

            fn run(
                &self,
                ctx: &$crate::phases::Context,
            ) -> ::anyhow::Result<$crate::phases::TaskResult> {
                let $items_ctx = ctx;
                let items: Vec<_> = { $items_expr };
                if items.is_empty() {
                    return Ok($crate::phases::TaskResult::NotApplicable(
                        "nothing configured".to_string(),
                    ));
                }
                $(
                    let $setup_ctx = ctx;
                    { $setup_expr }
                )?
                let resources = items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    $build_expr
                });
                $crate::phases::process_resources(ctx, resources, &$opts)
            }
        }
    };
}

pub(crate) use resource_task;
