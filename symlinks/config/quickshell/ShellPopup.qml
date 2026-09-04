pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Hyprland
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property int panelWidth: 368
    // Animate inside a stable Wayland buffer; resizing it mid-transition flickers.
    property real reservedContentHeight: availableContentHeight
    readonly property int shadowMargin: 8
    readonly property var anchorWindow: anchorItem ? anchorItem.QsWindow.window : null
    readonly property int availableContentHeight: Math.max(96, (anchorWindow && anchorWindow.screen ? anchorWindow.screen.height : 720) - (anchorWindow ? anchorWindow.height : Theme.barHeight) - Theme.spacing * 2 - Theme.padding * 2 - shadowMargin * 2)
    readonly property bool closing: closeAnimation.running
    default property alias contentData: body.data
    property real reveal: 0

    function close() {
        if (!visible || closing)
            return;
        openAnimation.stop();
        focusGrab.active = false;
        closeAnimation.start();
    }

    implicitWidth: Math.min(panelWidth, (anchorWindow && anchorWindow.screen ? anchorWindow.screen.width : panelWidth + 32) - 32) + shadowMargin * 2
    implicitHeight: Math.min(Math.max(body.implicitHeight, reservedContentHeight), availableContentHeight) + Theme.padding * 2 + shadowMargin * 2
    color: "transparent"
    visible: false
    grabFocus: false
    mask: Region {
        item: panel
    }

    anchor {
        window: root.anchorWindow
        adjustment: PopupAdjustment.SlideX | PopupAdjustment.SlideY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            if (!root.anchorWindow)
                return;
            const point = root.anchorWindow.contentItem.mapFromItem(root.anchorItem, root.anchorItem.width - root.width + root.shadowMargin, root.anchorItem.height + Theme.spacing - root.shadowMargin);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    onVisibleChanged: {
        if (visible) {
            closeAnimation.stop();
            reveal = 0;
            viewport.contentY = 0;
            openAnimation.restart();
            Qt.callLater(() => {
                if (root.visible && !root.closing) {
                    focusGrab.active = true;
                    contentFocus.forceActiveFocus(Qt.PopupFocusReason);
                }
            });
        } else {
            openAnimation.stop();
            closeAnimation.stop();
            focusGrab.active = false;
            reveal = 0;
        }
    }

    HyprlandFocusGrab {
        id: focusGrab
        // Whitelisting the bar lets another trigger switch menus in one click.
        windows: root.anchorWindow ? [root, root.anchorWindow] : [root]
        onCleared: root.close()
    }

    NumberAnimation {
        id: openAnimation
        target: root
        property: "reveal"
        to: 1
        duration: Theme.animationNormal
        easing.type: Easing.OutCubic
    }

    NumberAnimation {
        id: closeAnimation
        target: root
        property: "reveal"
        to: 0
        duration: Theme.animationFast
        easing.type: Easing.OutCubic
        onFinished: root.visible = false
    }

    FocusScope {
        id: contentFocus
        anchors.fill: parent
        opacity: root.reveal
        transform: Translate {
            y: -5 * (1 - root.reveal)
        }
        Keys.onEscapePressed: event => {
            root.close();
            event.accepted = true;
        }

        Repeater {
            model: 4
            Rectangle {
                required property int index
                anchors.fill: panel
                anchors.margins: -(index + 1) * 2
                radius: Theme.menuRadius + (index + 1) * 2
                color: "#09000000"
            }
        }

        MenuPanel {
            id: panel
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: root.shadowMargin
            height: Math.min(body.implicitHeight, root.availableContentHeight) + Theme.padding * 2

            Flickable {
                id: viewport
                anchors.fill: parent
                anchors.margins: Theme.padding
                contentHeight: body.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                interactive: contentHeight > height
                clip: true
                ScrollBar.vertical: ShellScrollBar {}

                Item {
                    id: body
                    width: viewport.width
                    implicitHeight: childrenRect.height
                    height: implicitHeight
                }
            }
        }
    }
}
