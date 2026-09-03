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

    implicitWidth: 324
    implicitHeight: 300
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
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 8);
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

    MenuPanel {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 6

            MenuHeader {
                Layout.fillWidth: true
                Layout.leftMargin: 2
                Layout.rightMargin: 2
                Layout.bottomMargin: 4
                icon: "\uf011"
                title: "Power"
                subtitle: "Session and system controls"
                accentColor: Theme.purple
                accentBackground: Theme.purpleSoft
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf023"
                label: "Lock"
                detail: "Keep the session running"
                showChevron: false
                onTriggered: root.run([Quickshell.env("HOME") + "/.config/hypr/scripts/lock-screen.sh"])
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf2f5"
                label: root.confirmation === "logout" ? "Click again to log out" : "Log out"
                detail: root.confirmation === "logout" ? "Confirm within 3 seconds" : "End this Hyprland session"
                danger: root.confirmation === "logout"
                selected: root.confirmation === "logout"
                showChevron: false
                onTriggered: root.request("logout", ["hyprctl", "dispatch", "hl.dsp.exit()"])
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf2f1"
                label: root.confirmation === "reboot" ? "Click again to restart" : "Restart"
                detail: root.confirmation === "reboot" ? "Confirm within 3 seconds" : "Reboot the computer"
                danger: root.confirmation === "reboot"
                selected: root.confirmation === "reboot"
                showChevron: false
                onTriggered: root.request("reboot", ["systemctl", "reboot"])
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf011"
                label: root.confirmation === "shutdown" ? "Click again to shut down" : "Shut down"
                detail: root.confirmation === "shutdown" ? "Confirm within 3 seconds" : "Power off the computer"
                danger: true
                selected: root.confirmation === "shutdown"
                showChevron: false
                onTriggered: root.request("shutdown", ["systemctl", "poweroff"])
            }
        }
    }
}
