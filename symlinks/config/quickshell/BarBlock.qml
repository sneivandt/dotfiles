import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

AbstractButton {
    id: root

    property string glyph: ""
    property string tooltip: ""
    property color textColor: Theme.foreground
    property bool interactive: true
    property bool selected: false
    readonly property real naturalTextWidth: label.implicitWidth
    readonly property bool textTruncated: label.truncated

    signal activated(int button)
    signal scrolled(int steps)

    implicitWidth: content.implicitWidth + horizontalPadding * 2
    implicitHeight: Theme.barControlHeight
    horizontalPadding: 10
    leftPadding: horizontalPadding
    rightPadding: horizontalPadding
    topPadding: 0
    bottomPadding: 0
    hoverEnabled: interactive
    focusPolicy: interactive ? Qt.StrongFocus : Qt.NoFocus
    Accessible.name: tooltip.length > 0 ? tooltip : text
    onClicked: {
        if (interactive) {
            activated(Qt.LeftButton);
        }
    }
    Keys.onReturnPressed: {
        if (interactive) {
            activated(Qt.LeftButton);
        }
    }
    Keys.onEnterPressed: {
        if (interactive) {
            activated(Qt.LeftButton);
        }
    }

    MouseArea {
        anchors.fill: parent
        enabled: root.interactive
        acceptedButtons: Qt.MiddleButton | Qt.RightButton
        onClicked: (event) => {
            return root.activated(event.button);
        }
        onWheel: (event) => {
            if (event.angleDelta.y !== 0)
                root.scrolled(event.angleDelta.y > 0 ? 1 : -1);

        }
    }

    ShellToolTip {
        text: root.tooltip
        visible: root.tooltip.length > 0 && (root.hovered || root.visualFocus) && !root.selected && !root.down
    }

    HoverHandler {
        cursorShape: root.interactive ? Qt.PointingHandCursor : Qt.ArrowCursor
    }

    background: Rectangle {
        radius: Theme.controlRadius
        color: root.interactive && root.down ? Theme.pressed : (root.selected ? Theme.selected : (root.hovered ? Theme.hover : "transparent"))
        border.width: root.visualFocus ? 1 : 0
        border.color: Theme.blue

        Rectangle {
            anchors.bottom: parent.bottom
            anchors.horizontalCenter: parent.horizontalCenter
            width: root.selected ? Math.min(18, parent.width - 10) : 0
            height: 2
            radius: 1
            color: Theme.blue

            Behavior on width {
                NumberAnimation {
                    duration: Theme.animationNormal
                    easing.type: Easing.OutCubic
                }

            }

        }

        Behavior on color {
            ColorAnimation {
                duration: Theme.animationFast
            }

        }

    }

    contentItem: RowLayout {
        id: content

        spacing: 8

        Text {
            visible: root.glyph.length > 0
            Layout.preferredWidth: 16
            text: root.glyph
            color: root.selected ? Theme.blue : root.textColor
            font.family: Theme.iconFont
            font.pixelSize: 14
            style: Text.Raised
            styleColor: Theme.textShadow
            horizontalAlignment: Text.AlignHCenter
        }

        Text {
            id: label

            visible: root.text.length > 0
            Layout.fillWidth: true
            text: root.text
            textFormat: Text.PlainText
            color: root.selected ? Theme.blue : root.textColor
            font.family: Theme.font
            font.pixelSize: Theme.textBody
            style: Text.Raised
            styleColor: Theme.textShadow
            elide: Text.ElideRight
        }

    }

}
