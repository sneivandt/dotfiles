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
    readonly property var shownWorkspaceIds: {
        const monitor = activeWorkspace ? activeWorkspace.monitor : null;
        const ids = workspaces.filter(workspace => workspace.id >= 1 && workspace.id <= 9 && (workspace.id === activeId || workspace.toplevels.values.length > 0 || (workspace.active && (!monitor || (workspace.monitor && workspace.monitor.id !== monitor.id))))).map(workspace => workspace.id);
        // The monitor can switch before the workspace model includes its new entry.
        if (activeId >= 1 && activeId <= 9 && !ids.includes(activeId))
            ids.push(activeId);
        return ids.sort((a, b) => a - b);
    }
    readonly property string titleText: activeToplevel && activeToplevel.workspace && activeToplevel.workspace.id === activeId ? activeToplevel.title : ""
    readonly property real workspaceWidth: shownWorkspaceIds.length * Theme.barControlHeight
    readonly property real titleWidth: titleText.length > 0 && titleWidthBudget >= 80 ? Math.min(Math.ceil(title.naturalTextWidth) + title.horizontalPadding * 2, titleWidthBudget) : 0

    signal workspaceRequested(int number)

    // Derive geometry from state, not a child layout's deferred implicit-size pass.
    implicitWidth: workspaceWidth + titleWidth

    Row {
        width: root.workspaceWidth
        height: parent.height

        Repeater {
            model: 9

            BarBlock {
                id: workspaceButton
                required property int index
                readonly property int number: index + 1
                readonly property bool current: root.activeId === number
                visible: root.shownWorkspaceIds.includes(number)
                width: Theme.barControlHeight
                horizontalPadding: 0
                tooltip: "Workspace " + number
                onActivated: root.workspaceRequested(number)
                contentItem: Item {
                    Rectangle {
                        anchors.centerIn: parent
                        width: workspaceButton.current ? 14 : 6
                        height: 6
                        radius: 3
                        color: workspaceButton.current ? Theme.blue : Theme.mutedStrong
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
