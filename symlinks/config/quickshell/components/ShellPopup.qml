pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Hyprland
import "../lib/Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property int panelWidth: 368
    property bool alignToWindowFrame: false
    property bool dismissOnEscape: true
    // Animate inside a stable Wayland buffer; resizing it mid-transition flickers.
    property real reservedContentHeight: availableContentHeight
    readonly property int shadowMargin: 9
    readonly property int topOffset: Theme.windowGap
    readonly property int screenInset: Theme.windowGap + shadowMargin
    readonly property var anchorWindow: anchorItem ? anchorItem.QsWindow.window : null
    readonly property int availableContentHeight: Math.max(96, (anchorWindow && anchorWindow.screen ? anchorWindow.screen.height : 720) - (anchorWindow ? anchorWindow.height : Theme.barHeight) - topOffset - Theme.windowGap - Theme.padding * 2 - shadowMargin * 2)
    readonly property bool closing: closeAnimation.running
    default property alias contentData: body.data
    property real reveal: 0

    signal escapePressed()

    function close() {
        if (!visible || closing)
            return;
        openAnimation.stop();
        focusGrab.active = false;
        closeAnimation.start();
    }

    implicitWidth: Math.min(panelWidth, (anchorWindow && anchorWindow.screen ? anchorWindow.screen.width : panelWidth + screenInset * 2) - screenInset * 2) + shadowMargin * 2
    implicitHeight: Math.min(Math.max(body.implicitHeight, reservedContentHeight), availableContentHeight) + Theme.padding * 2 + shadowMargin * 2
    color: "transparent"
    visible: false
    grabFocus: false
    mask: Region {
        item: panel
    }

    anchor {
        window: root.anchorWindow
        adjustment: root.alignToWindowFrame ? PopupAdjustment.SlideY : PopupAdjustment.SlideX | PopupAdjustment.SlideY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            if (!root.anchorWindow)
                return;
            const point = root.anchorWindow.contentItem.mapFromItem(root.anchorItem, root.anchorItem.width - root.width + root.shadowMargin, root.anchorItem.height + root.topOffset - root.shadowMargin);
            anchor.rect.x = root.alignToWindowFrame ? root.anchorWindow.contentItem.width - Theme.windowGap + Theme.windowBorderWidth - root.width + root.shadowMargin : point.x;
            anchor.rect.y = root.alignToWindowFrame ? root.anchorWindow.contentItem.height + Theme.windowGapTop - root.shadowMargin : point.y;
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
        scale: 0.985 + root.reveal * 0.015
        transformOrigin: Item.TopRight
        transform: Translate {
            y: -5 * (1 - root.reveal)
        }
        Keys.onEscapePressed: event => {
            if (root.dismissOnEscape)
                root.close();
            else
                root.escapePressed();
            event.accepted = true;
        }

        Repeater {
            model: 3
            Rectangle {
                required property int index
                readonly property int spread: index === 0 ? 2 : (index === 1 ? 5 : 8)
                anchors.fill: panel
                anchors.margins: -spread
                radius: Theme.menuRadius + spread
                color: index === 0 ? "#32000000" : (index === 1 ? "#1a000000" : "#0c000000")
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
