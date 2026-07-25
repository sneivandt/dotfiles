/// Implement [`Task::dependencies`](crate::engine::Task::dependencies) by expanding to the required
/// `fn dependencies(&self) -> &[TaskId]` method body.
///
/// The `const DEPS` intermediate is required because [`std::any::TypeId::of`]
/// is a `const fn` — placing it in a `const` ensures the slice has a
/// `'static` lifetime as required by the return type.  Each type is
/// wrapped in [`TaskId::Type`](crate::engine::TaskId::Type) automatically.
///
/// # Examples
///
/// ```ignore
/// task_deps![super::reload_config::ReloadConfig, super::symlinks::InstallSymlinks]
/// ```
macro_rules! task_deps {
    [$($dep:ty),+ $(,)?] => {
        fn dependencies(&self) -> &[$crate::engine::TaskId] {
            const DEPS: &[$crate::engine::TaskId] = &[
                $($crate::engine::TaskId::Type(std::any::TypeId::of::<$dep>())),+
            ];
            DEPS
        }
    };
}

pub(crate) use task_deps;

/// Implement common [`Task`](crate::engine::Task) metadata methods.
///
/// Use this for hand-written tasks whose body cannot use [`resource_task!`] but
/// whose name, optional update-only membership, and dependencies are static.
///
/// # Examples
///
/// ```ignore
/// task_metadata! {
///     name: "Install packages",
///     selector: "packages",
///     deps: [InstallParu],
/// }
/// ```
macro_rules! task_metadata {
    (
        name: $task_name:expr,
        $(selector: $selector:expr,)?
        $(visibility: $visibility:expr,)?
        $(update_only: $update_only:expr,)?
        $(deps: [$($dep:ty),+ $(,)?],)?
    ) => {
        fn name(&self) -> &'static str {
            $task_name
        }

        $(
            fn selector(&self) -> &'static str {
                $selector
            }
        )?

        $(
            fn visibility(&self) -> $crate::engine::TaskVisibility {
                $visibility
            }
        )?

        $(
            fn update_only(&self) -> bool {
                $update_only
            }
        )?

        $($crate::engine::task_deps![$($dep),+];)?
    };
}

pub(crate) use task_metadata;

/// Announce the start of a task stage.
///
/// `announce` is `Some(name)` only when the task runs standalone; as part of a
/// larger command the surrounding runner has already announced it.
fn emit_task_stage(ctx: &crate::engine::Context, announce: Option<&'static str>) {
    if let Some(name) = announce {
        crate::infra::logging::Output::emit(
            ctx.log(),
            crate::infra::logging::MsgKind::TaskStage,
            ::std::borrow::Cow::Borrowed(name),
        );
    }
}

/// Run the body shared by every resource task: skip empty item lists, announce
/// the stage, then build and process one resource per configured item.
///
/// Keeping this in a normal function rather than in macro expansion means the
/// shared behaviour is written, type-checked, and debugged once instead of
/// once per [`resource_task!`] / [`config_resource_task!`] variant.
pub(crate) fn run_resource_task<Item, R>(
    ctx: &crate::engine::Context,
    announce: Option<&'static str>,
    items: Vec<Item>,
    mut build: impl FnMut(Item, &crate::engine::Context) -> R,
    opts: &crate::engine::ProcessOpts,
) -> ::anyhow::Result<Option<crate::engine::TaskResult>>
where
    R: crate::engine::IntrinsicState + Send,
{
    if items.is_empty() {
        return Ok(None);
    }
    emit_task_stage(ctx, announce);

    let resources = items.into_iter().map(|item| build(item, ctx));
    crate::engine::process_resources(ctx, resources, opts).map(Some)
}

/// Run a resource task whose state for every resource comes from one shared
/// query rather than from each resource individually.
pub(crate) fn run_batch_resource_task<Item, Cache, R>(
    ctx: &crate::engine::Context,
    announce: Option<&'static str>,
    items: Vec<Item>,
    mut build: impl FnMut(Item, &crate::engine::Context) -> R,
    load: impl Fn(&[R], &crate::engine::Context) -> ::anyhow::Result<Cache> + Sync,
    state: impl for<'a> Fn(&'a R, &Cache) -> ::anyhow::Result<crate::engine::ResourceState> + Sync,
    opts: &crate::engine::ProcessOpts,
) -> ::anyhow::Result<Option<crate::engine::TaskResult>>
where
    R: crate::engine::Resource + Send,
    Cache: Sync,
{
    if items.is_empty() {
        return Ok(None);
    }
    emit_task_stage(ctx, announce);
    ctx.debug_fmt(|| {
        format!(
            "batch-checking {} resources with a single query",
            items.len()
        )
    });

    let resources: Vec<R> = items.into_iter().map(|item| build(item, ctx)).collect();
    let cache = load(&resources, ctx)?;
    let provider = crate::engine::CachedStateProvider::new(&cache, state);
    crate::engine::process_resources_with_provider(ctx, resources, &provider, opts).map(Some)
}

/// Convert an optional configured-task result into the direct [`Task::run`] result.
pub(crate) fn configured_task_result(
    result: Option<crate::engine::TaskResult>,
) -> crate::engine::TaskResult {
    result.unwrap_or_else(|| {
        crate::engine::TaskResult::NotApplicable("nothing configured".to_string())
    })
}

/// Declare the struct and constructor shared by [`config_resource_task!`] arms.
#[doc(hidden)]
#[macro_export]
macro_rules! __config_task_struct {
    ($(#[$meta:meta])* $vis:vis $name:ident, $cfg_ty:ty) => {
        $(#[$meta])*
        #[derive(Debug)]
        $vis struct $name {
            config: $crate::infra::ConfigHandle<$cfg_ty>,
        }

        impl $name {
            /// Create the task with a handle to its configuration slice.
            #[must_use]
            pub const fn new(config: $crate::infra::ConfigHandle<$cfg_ty>) -> Self {
                Self { config }
            }
        }
    };
}

/// Implement the shared task contract for resource-task macros.
#[doc(hidden)]
#[macro_export]
macro_rules! __impl_resource_task {
    (
        $name:ident {
            name: $task_name:expr,
            $(selector: $selector:expr,)?
            $(visibility: $visibility:expr,)?
            $(update_only: $update_only:expr,)?
            $(deps: [$($dep:ty),+ $(,)?],)?
            $(guard: |$guard_self:ident, $guard_ctx:ident| $guard_expr:expr,)?
            run: |$run_self:ident, $run_ctx:ident, $emit_stage:ident| $run_expr:expr $(,)?
        }
    ) => {
        impl $crate::engine::Task for $name {
            fn name(&self) -> &'static str {
                $task_name
            }

            $(
            fn selector(&self) -> &'static str {
                $selector
            }
            )?

            $(
            fn visibility(&self) -> $crate::engine::TaskVisibility {
                $visibility
            }
            )?

            $(
            fn update_only(&self) -> bool {
                $update_only
            }
            )?

            $($crate::engine::task_deps![$($dep),+];)?

            $(
            fn should_run(&self, ctx: &$crate::engine::Context) -> bool {
                let $guard_self = self;
                let $guard_ctx = ctx;
                $guard_expr
            }
            )?

            fn run_configured(
                &self,
                ctx: &$crate::engine::Context,
            ) -> ::anyhow::Result<Option<$crate::engine::TaskResult>> {
                let $run_self = self;
                let $run_ctx = ctx;
                let $emit_stage = true;
                $run_expr
            }

            fn run(
                &self,
                ctx: &$crate::engine::Context,
            ) -> ::anyhow::Result<$crate::engine::TaskResult> {
                let $run_self = self;
                let $run_ctx = ctx;
                let $emit_stage = false;
                Ok($crate::engine::configured_task_result($run_expr?))
            }
        }
    };
}

/// Define a task that reads config items, builds resources, and processes them.
///
/// Supports the standard intrinsic-state path and a batch path (`cache:` +
/// `state:`) for resources whose current state comes from one shared query.
/// Optional `update_only`, `deps`, and `guard` clauses cover the common task
/// variations without hand-writing [`Task`](crate::engine::Task) metadata.
///
/// Each arm only wires closures: the shared task body lives in
/// [`run_resource_task`] and [`run_batch_resource_task`].
macro_rules! resource_task {
    // -----------------------------------------------------------------
    // Batch variant — `cache:` and `state:` blocks are present.
    // -----------------------------------------------------------------
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            name: $task_name:expr,
            $(selector: $selector:expr,)?
            $(visibility: $visibility:expr,)?
            $(update_only: $update_only:expr,)?
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

        $crate::__impl_resource_task! {
            $name {
                name: $task_name,
                $(selector: $selector,)?
                $(visibility: $visibility,)?
                $(update_only: $update_only,)?
                $(deps: [$($dep),+],)?
                $(guard: |_task, $guard_ctx| $guard_expr,)?
                run: |_task, ctx, emit_stage| $crate::engine::run_batch_resource_task(
                    ctx,
                    emit_stage.then_some($task_name),
                    { let $items_ctx = ctx; $items_expr },
                    |$item, $build_ctx| $build_expr,
                    |$cache_items, $cache_ctx| $cache_expr,
                    |$state_res, $state_cache| Ok($state_expr),
                    &$opts,
                ),
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
            $(selector: $selector:expr,)?
            $(visibility: $visibility:expr,)?
            $(update_only: $update_only:expr,)?
            $(deps: [$($dep:ty),+ $(,)?],)?
            $(guard: |$guard_ctx:ident| $guard_expr:expr,)?
            items: |$items_ctx:ident| $items_expr:expr,
            build: |$item:ident, $build_ctx:ident| $build_expr:expr,
            opts: $opts:expr $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug)]
        $vis struct $name;

        $crate::__impl_resource_task! {
            $name {
                name: $task_name,
                $(selector: $selector,)?
                $(visibility: $visibility,)?
                $(update_only: $update_only,)?
                $(deps: [$($dep),+],)?
                $(guard: |_task, $guard_ctx| $guard_expr,)?
                run: |_task, ctx, emit_stage| $crate::engine::run_resource_task(
                    ctx,
                    emit_stage.then_some($task_name),
                    { let $items_ctx = ctx; $items_expr },
                    |$item, $build_ctx| $build_expr,
                    &$opts,
                ),
            }
        }
    };
}

pub(crate) use resource_task;

/// Like [`resource_task!`] but for tasks whose item list is backed by a typed
/// [`ConfigHandle`](crate::infra::ConfigHandle) rather than read from the
/// execution context.
///
/// The generated struct owns a `ConfigHandle<$config>` and a `new(handle)`
/// constructor.  The `items` and `guard` closures receive a borrow of the
/// current config snapshot (`&$config`) instead of the [`Context`](crate::engine::Context),
/// keeping the task decoupled from the aggregate application configuration.
macro_rules! config_resource_task {
    // -----------------------------------------------------------------
    // Batch variant — `cache:` and `state:` blocks are present.
    // -----------------------------------------------------------------
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            name: $task_name:expr,
            $(selector: $selector:expr,)?
            $(visibility: $visibility:expr,)?
            $(update_only: $update_only:expr,)?
            config: $cfg_ty:ty,
            $(deps: [$($dep:ty),+ $(,)?],)?
            $(guard: |$guard_cfg:ident, $guard_ctx:ident| $guard_expr:expr,)?
            items: |$items_cfg:ident| $items_expr:expr,
            cache: |$cache_items:ident, $cache_ctx:ident| $cache_expr:expr,
            build: |$item:ident, $build_ctx:ident| $build_expr:expr,
            state: |$state_res:ident, $state_cache:ident| $state_expr:expr,
            opts: $opts:expr $(,)?
        }
    ) => {
        $crate::__config_task_struct! { $(#[$meta])* $vis $name, $cfg_ty }

        $crate::__impl_resource_task! {
            $name {
                name: $task_name,
                $(selector: $selector,)?
                $(visibility: $visibility,)?
                $(update_only: $update_only,)?
                $(deps: [$($dep),+],)?
                $(guard: |task, $guard_ctx| {
                    let snapshot = task.config.read();
                    let $guard_cfg = &*snapshot;
                    $guard_expr
                },)?
                run: |task, ctx, emit_stage| $crate::engine::run_batch_resource_task(
                    ctx,
                    emit_stage.then_some($task_name),
                    { let snapshot = task.config.read(); let $items_cfg = &*snapshot; $items_expr },
                    |$item, $build_ctx| $build_expr,
                    |$cache_items, $cache_ctx| $cache_expr,
                    |$state_res, $state_cache| Ok($state_expr),
                    &$opts,
                ),
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
            $(selector: $selector:expr,)?
            $(visibility: $visibility:expr,)?
            $(update_only: $update_only:expr,)?
            config: $cfg_ty:ty,
            $(deps: [$($dep:ty),+ $(,)?],)?
            $(guard: |$guard_cfg:ident, $guard_ctx:ident| $guard_expr:expr,)?
            items: |$items_cfg:ident| $items_expr:expr,
            build: |$item:ident, $build_ctx:ident| $build_expr:expr,
            opts: $opts:expr $(,)?
        }
    ) => {
        $crate::__config_task_struct! { $(#[$meta])* $vis $name, $cfg_ty }

        $crate::__impl_resource_task! {
            $name {
                name: $task_name,
                $(selector: $selector,)?
                $(visibility: $visibility,)?
                $(update_only: $update_only,)?
                $(deps: [$($dep),+],)?
                $(guard: |task, $guard_ctx| {
                    let snapshot = task.config.read();
                    let $guard_cfg = &*snapshot;
                    $guard_expr
                },)?
                run: |task, ctx, emit_stage| $crate::engine::run_resource_task(
                    ctx,
                    emit_stage.then_some($task_name),
                    { let snapshot = task.config.read(); let $items_cfg = &*snapshot; $items_expr },
                    |$item, $build_ctx| $build_expr,
                    &$opts,
                ),
            }
        }
    };
}

pub(crate) use config_resource_task;
