import QtQuick
import QtQuick.Layouts
import "../lib/Theme.js" as Theme

Rectangle {
    id: root

    property string text: ""
    property string tone: "info"
    property string icon: ""
    property bool spinning: false
    property int maximumLineCount: 0
    property alias labelObjectName: message.objectName

    readonly property color toneColor: tone === "error" ? Theme.red : (tone === "warning" ? Theme.yellow : Theme.blue)
    readonly property color toneBackground: tone === "error" ? Theme.redSoft : (tone === "warning" ? Theme.yellowSoft : Theme.blueSoft)
    readonly property string resolvedIcon: icon.length > 0 ? icon : (spinning ? "\uf110" : (tone === "info" ? "\uf05a" : "\uf071"))

    implicitHeight: layout.implicitHeight + 16
    radius: Theme.controlRadius
    color: toneBackground
    border.width: 1
    border.color: Qt.rgba(toneColor.r, toneColor.g, toneColor.b, 0.35)
    Accessible.role: tone === "error" ? Accessible.AlertMessage : Accessible.StaticText
    Accessible.name: text

    RowLayout {
        id: layout

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        spacing: Theme.spacing

        Text {
            id: bannerIcon

            Layout.alignment: Qt.AlignTop
            Layout.topMargin: 1
            text: root.resolvedIcon
            font.family: Theme.iconFont
            font.pixelSize: Theme.textSmall
            color: root.toneColor

            NumberAnimation on rotation {
                from: 0
                to: 360
                duration: 1200
                loops: Animation.Infinite
                running: root.visible && root.spinning
                onRunningChanged: {
                    if (!running)
                        bannerIcon.rotation = 0;
                }
            }
        }

        Text {
            id: message

            Layout.fillWidth: true
            text: root.text
            textFormat: Text.PlainText
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
            color: root.tone === "error" ? Theme.red : (root.tone === "warning" ? Theme.yellow : Theme.foreground)
            wrapMode: Text.Wrap
            maximumLineCount: root.maximumLineCount > 0 ? root.maximumLineCount : 1000
            elide: root.maximumLineCount > 0 ? Text.ElideRight : Text.ElideNone
            Accessible.role: root.tone === "error" ? Accessible.AlertMessage : Accessible.StaticText
        }
    }
}
