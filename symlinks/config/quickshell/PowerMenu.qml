import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property string confirmation: ""

    function run(command) {
        actionProcess.exec(command);
        visible = false;
    }

    function request(action, command) {
        if (confirmation === action) {
            confirmation = "";
            run(command);
        } else {
            confirmation = action;
            confirmationTimer.restart();
        }
    }

    implicitWidth: 292
    implicitHeight: 239
    color: "transparent"
    grabFocus: true
    onVisibleChanged: {
        if (!visible)
            confirmation = "";

    }

    anchor {
        window: root.anchorItem.QsWindow.window
        adjustment: PopupAdjustment.SlideX | PopupAdjustment.FlipY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            const content = root.anchorItem.QsWindow.contentItem;
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 6);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    Process {
        id: actionProcess
    }

    Timer {
        id: confirmationTimer

        interval: 3000
        onTriggered: root.confirmation = ""
    }

    Rectangle {
        anchors.fill: parent
        radius: 10
        color: Theme.backgroundSolid
        border.width: 1
        border.color: Theme.border

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 3

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 5
                Layout.rightMargin: 5
                Layout.bottomMargin: 5

                Text {
                    text: "Power"
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: 16
                    font.weight: Font.DemiBold
                }
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf023"
                label: "Lock"
                detail: "Keep the session running"
                onTriggered: root.run([Quickshell.env("HOME") + "/.config/hypr/scripts/lock-screen.sh"])
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf2f5"
                label: root.confirmation === "logout" ? "Click again to log out" : "Log out"
                detail: "End this Hyprland session"
                danger: root.confirmation === "logout"
                onTriggered: root.request("logout", ["hyprctl", "dispatch", "hl.dsp.exit()"])
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf2f1"
                label: root.confirmation === "reboot" ? "Click again to restart" : "Restart"
                detail: "Reboot the computer"
                danger: root.confirmation === "reboot"
                onTriggered: root.request("reboot", ["systemctl", "reboot"])
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf011"
                label: root.confirmation === "shutdown" ? "Click again to shut down" : "Shut down"
                detail: "Power off the computer"
                danger: true
                onTriggered: root.request("shutdown", ["systemctl", "poweroff"])
            }
        }
    }
}
