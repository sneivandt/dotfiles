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
    property bool _actionActive: false
    readonly property var actions: ({
            logout: {
                label: "Log out",
                command: ["uwsm", "stop"]
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
        if (_actionActive || actionProcess.running)
            return;
        error = "";
        _actionActive = true;
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
        objectName: "powerActionProcess"
        onRunningChanged: {
            if (!running && root._actionActive) {
                root._actionActive = false;
                root.error = "Cannot start the action. Check that its command is installed and executable.";
            }
        }
        stderr: StdioCollector {
            id: actionErrors
        }
        onExited: code => {
            if (!root._actionActive)
                return;
            root._actionActive = false;
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
            enabled: !root._actionActive

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
                enabled: !root._actionActive
                onTriggered: root.confirmation = ""
            }
            MenuButton {
                Layout.fillWidth: true
                label: root._actionActive ? "Working..." : (root.confirmation ? root.actions[root.confirmation].label : "")
                danger: true
                selected: true
                showSelectionIndicator: false
                showChevron: false
                enabled: !root._actionActive
                onTriggered: root.run(root.actions[root.confirmation].command)
            }
        }
        Text {
            visible: root.error.length > 0
            Layout.fillWidth: true
            text: root.error
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: Theme.red
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
        }
    }
}
