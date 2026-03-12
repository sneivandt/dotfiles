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
/// // expands to:
/// //   fn dependencies(&self) -> &[std::any::TypeId] {
/// //       const DEPS: &[std::any::TypeId] = &[
/// //           std::any::TypeId::of::<super::reload_config::ReloadConfig>(),
/// //           std::any::TypeId::of::<super::symlinks::InstallSymlinks>(),
/// //       ];
/// //       DEPS
/// //   }
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
/// # Syntax
///
/// ```ignore
/// resource_task! {
///     /// Doc comment for the task.
///     pub StructName {
///         name: "Human-readable task name",
///         deps: [DepType1, DepType2],          // optional
///         guard: |ctx| bool_expr,              // optional platform/tool guard
///         setup: |ctx| { side_effects(); },    // optional pre-processing
///         items: |ctx| ctx.config_read().field.clone(),
///         build: |item, ctx| Resource::from(&item, &ctx.home),
///         opts: ProcessOpts::strict("verb"),
///     }
/// }
/// ```
///
/// The generated struct implements `Task` with:
/// - `should_run` returning `false` only when the guard fails
/// - `run_if_applicable` evaluating items exactly once per task execution and
///   returning `None` when no items are configured
/// - `run` returning [`TaskResult::NotApplicable`] when the guard fails or no
///   items are configured, otherwise running the optional setup block, mapping
///   items to resources via `build`, and delegating to [`process_resources`]
macro_rules! resource_task {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            name: $task_name:expr,
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

            $($crate::tasks::task_deps![$($dep),+];)?

            fn should_run(&self, ctx: &$crate::tasks::Context) -> bool {
                let _ = ctx;
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
                $(
                    let $setup_ctx = ctx;
                    { $setup_expr }
                )?
                let resources = items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    $build_expr
                });
                $crate::tasks::process_resources(ctx, resources, &$opts).map(Some)
            }

            fn run(&self, ctx: &$crate::tasks::Context) -> ::anyhow::Result<$crate::tasks::TaskResult> {
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
                let resources = items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    $build_expr
                });
                $crate::tasks::process_resources(ctx, resources, &$opts)
            }
        }
    };
}

pub(crate) use resource_task;

/// Define a task that batch-queries state once and then processes resources
/// with pre-computed states.
///
/// This is the counterpart to [`resource_task!`] for resources whose state
/// is determined by a single bulk query (e.g., VS Code extensions, registry
/// entries).
///
/// # Syntax
///
/// ```ignore
/// batch_resource_task! {
///     /// Doc comment for the task.
///     pub StructName {
///         name: "Human-readable task name",
///         deps: [DepType1, DepType2],               // optional
///         guard: |ctx| bool_expr,                    // optional
///         items: |ctx| ctx.config_read().field.clone(),
///         cache: |items, ctx| query_bulk_state(items, ctx),
///         build: |item, ctx| Resource::from(&item, &ctx.home),
///         state: |resource, cache| resource.state_from_cache(&cache),
///         opts: ProcessOpts::lenient("verb"),
///     }
/// }
/// ```
///
/// The generated struct implements `Task` with:
/// - `should_run` returning `false` only when the guard fails
/// - `run_if_applicable` evaluating items exactly once per task execution and
///   returning `None` when no items are configured
/// - `run` returning [`TaskResult::NotApplicable`] when the guard fails or no
///   items are configured, otherwise querying bulk state via `cache`, building
///   `(Resource, ResourceState)` pairs, and delegating to
///   [`process_resource_states`]
macro_rules! batch_resource_task {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            name: $task_name:expr,
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

        impl $crate::tasks::Task for $name {
            fn name(&self) -> &'static str {
                $task_name
            }

            $($crate::tasks::task_deps![$($dep),+];)?

            fn should_run(&self, ctx: &$crate::tasks::Context) -> bool {
                let _ = ctx;
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
                let $cache_items: Vec<_> = { $items_expr };
                if $cache_items.is_empty() {
                    return Ok(None);
                }
                ctx.log.debug(&format!(
                    "batch-checking {} resources with a single query",
                    $cache_items.len()
                ));
                let $cache_ctx = ctx;
                let $state_cache = { $cache_expr }?;
                let resource_states = $cache_items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    let $state_res = { $build_expr };
                    let state = { $state_expr };
                    ($state_res, state)
                });
                $crate::tasks::process_resource_states(ctx, resource_states, &$opts).map(Some)
            }

            fn run(&self, ctx: &$crate::tasks::Context) -> ::anyhow::Result<$crate::tasks::TaskResult> {
                let $items_ctx = ctx;
                let $cache_items: Vec<_> = { $items_expr };
                if $cache_items.is_empty() {
                    return Ok($crate::tasks::TaskResult::NotApplicable(
                        "nothing configured".to_string(),
                    ));
                }
                ctx.log.debug(&format!(
                    "batch-checking {} resources with a single query",
                    $cache_items.len()
                ));
                let $cache_ctx = ctx;
                let $state_cache = { $cache_expr }?;
                let resource_states = $cache_items.into_iter().map(|$item| {
                    let $build_ctx = ctx;
                    let $state_res = { $build_expr };
                    let state = { $state_expr };
                    ($state_res, state)
                });
                $crate::tasks::process_resource_states(ctx, resource_states, &$opts)
            }
        }
    };
}

pub(crate) use batch_resource_task;
