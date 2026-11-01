import QtQuick
import QtQuick.Controls

ShellPopup {
    id: root
    required property var notifications
    readonly property bool isNotificationMenu: true
    panelWidth: 420

    Flickable {
        id: scroll
        width: parent.width
        height: Math.min(contentHeight, root.availableContentHeight)
        contentHeight: content.implicitHeight
        contentWidth: width
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        activeFocusOnTab: true
        Keys.onDownPressed: contentY = Math.min(Math.max(0, contentHeight - height), contentY + 58)
        Keys.onUpPressed: contentY = Math.max(0, contentY - 58)
        Keys.onPressed: event => {
            if (event.key === Qt.Key_PageDown) {
                contentY = Math.min(Math.max(0, contentHeight - height), contentY + height);
                event.accepted = true;
            } else if (event.key === Qt.Key_PageUp) {
                contentY = Math.max(0, contentY - height);
                event.accepted = true;
            }
        }
        ScrollBar.vertical: ShellScrollBar {}

        NotificationContent {
            id: content
            width: scroll.width
            state: root.notifications
        }
    }
    onVisibleChanged: {
        if (!visible)
            scroll.contentY = 0;
    }
}
