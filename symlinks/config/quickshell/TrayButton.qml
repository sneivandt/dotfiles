import QtQuick
import QtQuick.Controls
import Quickshell.Widgets
import "Theme.js" as Theme

AbstractButton {
    id: root

    required property var trayItem
    required property var anchorWindow
    required property var menuController
    property string customIcon: ""
    property string appName: ""
    property bool customActivation: false
    property string activationError: ""

    signal activationRequested()

    implicitWidth: Theme.barControlHeight
    implicitHeight: Theme.barControlHeight
    enabled: trayItem !== null
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    Accessible.name: appName || (trayItem ? trayItem.tooltipTitle || trayItem.title || trayItem.id : "")

    function showMenu() {
        if (!trayItem)
            return;
        menuController.dismiss();
        const point = anchorWindow.contentItem.mapFromItem(root, 0, height);
        trayItem.display(anchorWindow, point.x, point.y);
    }

    onClicked: {
        if (customActivation)
            activationRequested();
        else if (trayItem.onlyMenu)
            showMenu();
        else
            trayItem.activate();
    }
    Keys.onMenuPressed: showMenu()
    Keys.onReturnPressed: clicked()
    Keys.onEnterPressed: clicked()

    background: Rectangle {
        radius: Theme.controlRadius
        color: root.hovered || root.down ? Theme.hover : "transparent"
        border.width: root.visualFocus ? 1 : 0
        border.color: Theme.blue
    }
    contentItem: Item {
        IconImage {
            anchors.centerIn: parent
            width: 16
            height: width
            source: root.customIcon || (root.trayItem ? root.trayItem.icon : "")
        }
    }
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.MiddleButton | Qt.RightButton
        onClicked: event => {
            if (event.button === Qt.MiddleButton)
                root.trayItem.secondaryActivate();
            else
                root.showMenu();
        }
        onWheel: event => root.trayItem.scroll(event.angleDelta.y, false)
    }
    ShellToolTip {
        text: root.activationError ? root.Accessible.name + "\n" + root.activationError : root.Accessible.name
        visible: root.hovered || root.visualFocus
    }
    HoverHandler {
        cursorShape: Qt.PointingHandCursor
    }
}
