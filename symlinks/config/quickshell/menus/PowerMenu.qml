import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import "../components"
import "../lib/Theme.js" as Theme

ShellPopup {
    id: root
    panelWidth: 336
    dismissOnEscape: confirmation.length === 0
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
    readonly property var confirmationCopy: ({
            logout: {
                icon: "\uf2f5",
                title: "Log out of this session?",
                detail: "Open applications will close and return you to login."
            },
            reboot: {
                icon: "\uf2f1",
                title: "Restart this computer?",
                detail: "Unsaved work in open applications may be lost."
            },
            shutdown: {
                icon: "\uf011",
                title: "Shut down this computer?",
                detail: "Unsaved work in open applications may be lost."
            }
        })

    function run(command) {
        if (_actionActive || actionProcess.running)
            return;
        error = "";
        _actionActive = true;
        actionProcess.exec(command);
    }

    onEscapePressed: {
        if (!_actionActive)
            confirmation = "";
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
        ColumnLayout {
            visible: root.confirmation.length > 0
            Layout.fillWidth: true
            spacing: Theme.spacing

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: confirmCard.implicitHeight + 20
                radius: Theme.itemRadius
                color: Theme.raised
                border.width: 1
                border.color: Theme.borderSubtle

                RowLayout {
                    id: confirmCard

                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    spacing: 12

                    Rectangle {
                        Layout.preferredWidth: 32
                        Layout.preferredHeight: 32
                        Layout.alignment: Qt.AlignTop
                        radius: Theme.controlRadius
                        color: root.confirmation === "shutdown" ? Theme.redSoft : Theme.yellowSoft

                        Text {
                            anchors.centerIn: parent
                            text: root.confirmation ? root.confirmationCopy[root.confirmation].icon : ""
                            color: root.confirmation === "shutdown" ? Theme.red : Theme.yellow
                            font.family: Theme.iconFont
                            font.pixelSize: 14
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4

                        Text {
                            objectName: "powerConfirmationTitle"
                            Layout.fillWidth: true
                            text: root.confirmation ? root.confirmationCopy[root.confirmation].title : ""
                            textFormat: Text.PlainText
                            color: Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: Theme.textBody
                            font.weight: Font.DemiBold
                            wrapMode: Text.WordWrap
                        }

                        Text {
                            objectName: "powerConfirmationDetail"
                            Layout.fillWidth: true
                            text: root.confirmation ? root.confirmationCopy[root.confirmation].detail : ""
                            textFormat: Text.PlainText
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: Theme.textSmall
                            wrapMode: Text.WordWrap
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing
                MenuButton {
                    id: cancelButton
                    objectName: "powerCancelButton"
                    Layout.fillWidth: true
                    implicitHeight: 40
                    label: "Cancel"
                    centerLabel: true
                    showChevron: false
                    enabled: !root._actionActive
                    onTriggered: root.confirmation = ""
                }
                MenuButton {
                    objectName: "powerConfirmButton"
                    Layout.fillWidth: true
                    implicitHeight: 40
                    label: root._actionActive ? "Working..." : (root.confirmation ? root.actions[root.confirmation].label : "")
                    danger: true
                    selected: true
                    centerLabel: true
                    showSelectionIndicator: false
                    showChevron: false
                    enabled: !root._actionActive
                    onTriggered: root.run(root.actions[root.confirmation].command)
                }
            }
        }
        MenuBanner {
            visible: root.error.length > 0
            Layout.fillWidth: true
            tone: "error"
            text: root.error
        }
    }
}
