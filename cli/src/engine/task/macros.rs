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
/// task_deps![super::repository::UpdateRepository, super::symlinks::InstallSymlinks]
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
/// Use this for tasks that only need the standard metadata block but
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
        $(result_display: $result_display:expr,)?
        $(update_only: $update_only:expr,)?
        $(deps: [$($dep:ty),+ $(,)?],)?
    ) => {
        fn meta(&self) -> $crate::engine::TaskMeta<'_> {
            $crate::engine::TaskMeta::new($task_name)
                $(.with_selector($selector))?
                $(.with_visibility($visibility))?
                $(.with_result_display($result_display))?
                $(.with_update_only($update_only))?
        }

        $($crate::engine::task_deps![$($dep),+];)?
    };
}

pub(crate) use task_metadata;

/// Run the body shared by resource tasks: skip empty item lists, then build
/// and process one resource per configured item.
///
/// Keeping this in a normal function rather than in macro expansion means the
/// shared behaviour is written, type-checked, and debugged once, not per task.
pub(crate) fn run_resource_task<Item, R>(
    ctx: &crate::engine::Context,
    items: Vec<Item>,
    mut build: impl FnMut(Item, &crate::engine::Context) -> R,
    opts: &crate::engine::ProcessOpts,
) -> ::anyhow::Result<crate::engine::TaskResult>
where
    R: crate::engine::IntrinsicState + Send,
{
    if items.is_empty() {
        return Ok(crate::engine::TaskResult::NotApplicable(
            "nothing configured".to_string(),
        ));
    }

    let resources = items.into_iter().map(|item| build(item, ctx));
    crate::engine::process_resources(ctx, resources, opts)
}

/// Run a resource task whose state for every resource comes from one shared
/// query rather than from each resource individually.
pub(crate) fn run_batch_resource_task<Item, Cache, R>(
    ctx: &crate::engine::Context,
    items: Vec<Item>,
    mut build: impl FnMut(Item, &crate::engine::Context) -> R,
    load: impl Fn(&[R], &crate::engine::Context) -> ::anyhow::Result<Cache> + Sync,
    state: impl for<'a> Fn(&'a R, &Cache) -> crate::engine::ResourceResult<crate::engine::ResourceState>
    + Sync,
    opts: &crate::engine::ProcessOpts,
) -> ::anyhow::Result<crate::engine::TaskResult>
where
    R: crate::engine::Resource + Send,
    Cache: Sync,
{
    if items.is_empty() {
        return Ok(crate::engine::TaskResult::NotApplicable(
            "nothing configured".to_string(),
        ));
    }
    ctx.trace_fmt(|| {
        format!(
            "batch-checking {} resources with a single query",
            items.len()
        )
    });

    let resources: Vec<R> = items.into_iter().map(|item| build(item, ctx)).collect();
    let cache = load(&resources, ctx)?;
    crate::engine::process_resources_with_cache(ctx, resources, &cache, state, opts)
}
