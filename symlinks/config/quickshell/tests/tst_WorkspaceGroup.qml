import QtQuick
import QtTest
import ".." as Shell

Item {
    width: 800
    height: 60

    Component {
        id: groupComponent
        Shell.WorkspaceGroup {}
    }

    TestCase {
        name: "WorkspaceGroup"
        when: windowShown

        function workspace(id, monitor, occupied, active) {
            return {
                id: id,
                monitor: monitor,
                active: active,
                toplevels: {
                    values: occupied ? [
                        {}
                    ] : []
                }
            };
        }

        function makeGroup(workspaces, active, toplevel) {
            return createTemporaryObject(groupComponent, parent, {
                workspaces: workspaces,
                activeWorkspace: active,
                activeToplevel: toplevel,
                titleWidthBudget: 800
            });
        }

        function test_new_empty_workspace_does_not_temporarily_widen_group() {
            const monitor = {
                id: 0
            };
            const used = workspace(1, monitor, true, true);
            const empty = workspace(2, monitor, false, false);
            const title = {
                workspace: used,
                title: "Short title"
            };
            const group = makeGroup([used], used, title);
            const initialWidth = group.implicitWidth;

            group.workspaces = [used, empty];
            compare(group.shownWorkspaceIds, [1]);
            compare(group.implicitWidth, initialWidth);

            group.activeWorkspace = empty;
            compare(group.shownWorkspaceIds, [1, 2]);
            compare(group.titleText, "");
            compare(group.implicitWidth, 56);

            group.activeToplevel = null;
            compare(group.implicitWidth, 56);
            wait(200);
            compare(group.implicitWidth, 56);
        }

        function test_switch_between_empty_workspaces_never_adds_a_slot() {
            const monitor = {
                id: 0
            };
            const used = workspace(1, monitor, true, false);
            const oldEmpty = workspace(2, monitor, false, true);
            const newEmpty = workspace(3, monitor, false, false);
            const group = makeGroup([used, oldEmpty], oldEmpty, null);
            compare(group.implicitWidth, 56);

            group.workspaces = [used, oldEmpty, newEmpty];
            compare(group.implicitWidth, 56);
            group.activeWorkspace = newEmpty;
            compare(group.shownWorkspaceIds, [1, 3]);
            compare(group.implicitWidth, 56);
            group.workspaces = [used, newEmpty];
            compare(group.implicitWidth, 56);
            wait(200);
            compare(group.implicitWidth, 56);
        }

        function test_active_workspace_can_arrive_before_model_entry() {
            const monitor = {
                id: 0
            };
            const used = workspace(1, monitor, true, true);
            const empty = workspace(2, monitor, false, false);
            const group = makeGroup([used], used, {
                workspace: used,
                title: "Editor"
            });

            group.activeWorkspace = empty;
            compare(group.shownWorkspaceIds, [1, 2]);
            compare(group.implicitWidth, 56);
            group.workspaces = [used, empty];
            compare(group.implicitWidth, 56);
        }

        function test_other_monitors_active_empty_workspace_is_preserved() {
            const monitor = {
                id: 0
            };
            const used = workspace(1, monitor, true, true);
            const otherEmpty = workspace(4, {
                id: 1
            }, false, true);
            const retiring = workspace(2, monitor, false, true);
            const group = makeGroup([used, otherEmpty, retiring], used, null);
            compare(group.shownWorkspaceIds, [1, 4]);
        }

        function test_title_is_measured_without_waiting_for_layout() {
            const used = workspace(1, {
                id: 0
            }, true, true);
            const group = makeGroup([used], used, {
                workspace: used,
                title: "A"
            });
            verify(group.titleWidth > 20 && group.titleWidth < 40);
            compare(group.implicitWidth, 28 + group.titleWidth);

            group.activeToplevel = {
                workspace: used,
                title: "Long title ".repeat(20)
            };
            compare(group.titleWidth, 800);
            compare(group.implicitWidth, 828);

            group.titleWidthBudget = 100;
            compare(group.titleWidth, 100);
            group.titleWidthBudget = 79;
            compare(group.titleWidth, 0);
            compare(group.implicitWidth, 28);
        }

        function test_titles_that_fit_are_not_elided_data() {
            return [
                {
                    tag: "short",
                    title: "GitHub Copilot"
                },
                {
                    tag: "one-character",
                    title: "A"
                },
                {
                    tag: "longer-than-old-cap",
                    title: "Reviewing workspace changes in the desktop shell"
                }
            ];
        }

        function test_titles_that_fit_are_not_elided(data) {
            const used = workspace(1, {
                id: 0
            }, true, true);
            const group = makeGroup([used], used, {
                workspace: used,
                title: data.title
            });
            const title = findChild(group, "workspaceTitle");
            verify(title !== null);
            verify(group.titleWidth < group.titleWidthBudget);
            verify(waitForRendering(group));
            tryCompare(title, "textTruncated", false);
            if (data.tag === "longer-than-old-cap")
                verify(group.titleWidth > 320);
        }
    }
}
