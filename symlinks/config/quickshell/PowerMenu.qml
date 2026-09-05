import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import "Theme.js" as Theme

ShellPopup {
    id: root
    panelWidth: 336
    property string confirmation: ""
    property string error: ""
    readonly property var actions: ({
            logout: {
                label: "Log out",
                command: ["hyprctl", "dispatch", "hl.dsp.exit()"]
            },
            reboot: {
                label: "Restart",
                command: ["systemctl", "reboot"]
            },
            shutdown: {
                label: "Shut down",
                command: ["systemctl", "poweroff"]
            }
        })

    function run(command) {
        if (actionProcess.running)
            return;
        error = "";
        actionProcess.exec(command);
    }

    onVisibleChanged: {
        if (!visible) {
            confirmation = "";
            error = "";
        }
    }

    Process {
        id: actionProcess
        stderr: StdioCollector {
            id: actionErrors
        }
        onExited: code => {
            if (code === 0)
                root.close();
            else
                root.error = actionErrors.text.trim() || "The action could not be completed.";
        }
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.spacing

        MenuHeader {
            Layout.fillWidth: true
            Layout.bottomMargin: 8
            icon: "\uf011"
            title: "Power"
            subtitle: "Session and system"
        }
        ColumnLayout {
            visible: root.confirmation.length === 0
            Layout.fillWidth: true
            spacing: 0
            enabled: !actionProcess.running

            MenuButton {
                Layout.fillWidth: true
                glyph: "\uf023"
                label: "Lock"
                showChevron: false
                onTriggered: root.run([Quickshell.env("HOME") + "/.config/hypr/scripts/lock-screen.sh"])
            }
            MenuButton {
                Layout.fillWidth: true
                glyph: "\uf2f5"
                label: "Log out"
                showChevron: false
                onTriggered: {
                    root.confirmation = "logout";
                    cancelButton.forceActiveFocus(Qt.TabFocusReason);
                }
            }
            Rectangle {
                Layout.fillWidth: true
                Layout.topMargin: 8
                Layout.bottomMargin: 8
                implicitHeight: 1
                color: Theme.borderSubtle
            }
            MenuButton {
                Layout.fillWidth: true
                glyph: "\uf2f1"
                label: "Restart"
                showChevron: false
                onTriggered: {
                    root.confirmation = "reboot";
                    cancelButton.forceActiveFocus(Qt.TabFocusReason);
                }
            }
            MenuButton {
                Layout.fillWidth: true
                glyph: "\uf011"
                label: "Shut down"
                danger: true
                showChevron: false
                onTriggered: {
                    root.confirmation = "shutdown";
                    cancelButton.forceActiveFocus(Qt.TabFocusReason);
                }
            }
        }
        RowLayout {
            visible: root.confirmation.length > 0
            Layout.fillWidth: true
            spacing: 8
            MenuButton {
                id: cancelButton
                Layout.fillWidth: true
                label: "Cancel"
                showChevron: false
                enabled: !actionProcess.running
                onTriggered: root.confirmation = ""
            }
            MenuButton {
                Layout.fillWidth: true
                label: actionProcess.running ? "Working..." : (root.confirmation ? root.actions[root.confirmation].label : "")
                danger: true
                selected: true
                showChevron: false
                enabled: !actionProcess.running
                onTriggered: root.run(root.actions[root.confirmation].command)
            }
        }
        Text {
            visible: root.error.length > 0
            Layout.fillWidth: true
            text: root.error
            wrapMode: Text.Wrap
            color: Theme.red
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
        }
    }
}
