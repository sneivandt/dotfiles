pragma ComponentBehavior: Bound

import QtQuick
import "Theme.js" as Theme

BarGroup {
    id: root

    required property var workspaces
    required property var activeWorkspace
    required property var activeToplevel
    required property real titleWidthBudget
    readonly property int activeId: activeWorkspace ? activeWorkspace.id : 0

    function workspaceFor(number) {
        return workspaces.find(workspace => workspace && workspace.id === number) || null;
    }

    function isUrgent(workspace) {
        if (!workspace)
            return false;
        if (workspace.urgent === true)
            return true;
        const toplevels = workspace.toplevels && workspace.toplevels.values ? workspace.toplevels.values : [];
        return toplevels.some(toplevel => toplevel && toplevel.urgent === true);
    }

    readonly property var shownWorkspaceIds: {
        const monitor = activeWorkspace ? activeWorkspace.monitor : null;
        const ids = workspaces.filter(workspace => workspace && workspace.id >= 1 && workspace.id <= 9 && (workspace.id === activeId || workspace.toplevels.values.length > 0 || isUrgent(workspace) || (workspace.active && (!monitor || (workspace.monitor && workspace.monitor.id !== monitor.id))))).map(workspace => workspace.id);
        // The monitor can switch before the workspace model includes its new entry.
        if (activeId >= 1 && activeId <= 9 && !ids.includes(activeId))
            ids.push(activeId);
        return ids.sort((a, b) => a - b);
    }
    readonly property string titleText: activeToplevel && activeToplevel.workspace && activeToplevel.workspace.id === activeId ? activeToplevel.title : ""
    property real maximumWorkspaceWidth: shownWorkspaceIds.length * Theme.barControlHeight
    readonly property real workspaceWidth: workspaceStrip.implicitWidth
    readonly property real titleWidth: titleText.length > 0 && titleWidthBudget >= 80 ? Math.min(Math.ceil(title.naturalTextWidth) + title.horizontalPadding * 2, titleWidthBudget) : 0

    signal workspaceRequested(int number)

    // Derive geometry from state, not a child layout's deferred implicit-size pass.
    implicitWidth: workspaceWidth + titleWidth

    BarStrip {
        id: workspaceStrip
        objectName: "workspaceStrip"
        width: implicitWidth
        height: parent.height
        naturalWidth: root.shownWorkspaceIds.length * Theme.barControlHeight
        maximumWidth: root.maximumWorkspaceWidth
        sectionName: "Workspaces"

        function revealActive() {
            const index = root.shownWorkspaceIds.indexOf(root.activeId);
            if (index >= 0)
                reveal(index * Theme.barControlHeight, Theme.barControlHeight);
        }

        onWidthChanged: Qt.callLater(revealActive)

        Row {
            height: parent.height

            Repeater {
                model: 9

                BarBlock {
                    id: workspaceButton
                    onVisualFocusChanged: {
                        if (visualFocus)
                            workspaceStrip.reveal(x, width);
                    }
                    required property int index
                    readonly property int number: index + 1
                    readonly property var workspaceEntry: root.workspaceFor(number)
                    readonly property bool current: root.activeId === number
                    readonly property bool urgent: !current && root.isUrgent(workspaceEntry)
                    readonly property bool otherMonitor: !current && !urgent && workspaceEntry !== null && workspaceEntry.active === true
                    visible: root.shownWorkspaceIds.includes(number)
                    width: Theme.barControlHeight
                    horizontalPadding: 0
                    tooltip: "Workspace " + number + (urgent ? " (urgent)" : (otherMonitor ? " (other monitor)" : ""))
                    onActivated: root.workspaceRequested(number)
                    contentItem: Item {
                        Rectangle {
                            objectName: "workspaceIndicator" + workspaceButton.number
                            anchors.centerIn: parent
                            width: workspaceButton.current ? 16 : (workspaceButton.urgent || workspaceButton.otherMonitor ? 10 : 6)
                            height: 6
                            radius: 3
                            color: workspaceButton.current ? Theme.blue : (workspaceButton.urgent ? Theme.red : (workspaceButton.otherMonitor ? "transparent" : Theme.mutedStrong))
                            border.width: workspaceButton.otherMonitor ? 1.5 : 0
                            border.color: workspaceButton.otherMonitor ? Theme.blue : "transparent"
                            Behavior on width {
                                NumberAnimation {
                                    duration: Theme.animationNormal
                                    easing.type: Easing.OutCubic
                                }
                            }
                            Behavior on color {
                                ColorAnimation {
                                    duration: Theme.animationFast
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    onActiveIdChanged: Qt.callLater(workspaceStrip.revealActive)
    onShownWorkspaceIdsChanged: Qt.callLater(workspaceStrip.revealActive)

    BarBlock {
        id: title
        objectName: "workspaceTitle"
        x: root.workspaceWidth
        width: root.titleWidth
        visible: width > 0
        text: root.titleText
        tooltip: text
        interactive: false
        textColor: Theme.mutedStrong
    }
}
