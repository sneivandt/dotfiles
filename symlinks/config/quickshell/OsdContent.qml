import QtQuick
import QtQuick.Layouts
import "Theme.js" as Theme

MenuPanel {
    id: root

    property string kind: "volume"
    property int percent: 0
    property bool muted: false

    readonly property bool isVolume: kind === "volume"
    readonly property bool isBrightness: kind === "brightness"
    readonly property bool isMicrophone: kind === "microphone"
    readonly property int maxPercent: isVolume ? 150 : 100
    readonly property int clampedPercent: Math.max(0, Math.min(maxPercent, percent))
    readonly property real fillFraction: muted ? 0 : clampedPercent / maxPercent
    readonly property bool amplified: isVolume && !muted && clampedPercent > 100
    readonly property string labelText: isBrightness ? "Brightness" : (isMicrophone ? "Microphone" : "Volume")
    readonly property string valueText: muted ? "Muted" : clampedPercent + "%"
    readonly property string iconGlyph: {
        if (isBrightness)
            return "\uf185";
        if (isMicrophone)
            return muted ? "\uf131" : "\uf130";
        if (muted || clampedPercent === 0)
            return "\uf6a9";
        return clampedPercent < 50 ? "\uf027" : "\uf028";
    }
    readonly property color accentColor: {
        if (muted)
            return Theme.mutedStrong;
        if (isBrightness)
            return Theme.yellow;
        if (amplified)
            return Theme.purple;
        return Theme.blue;
    }

    implicitWidth: 264
    implicitHeight: layout.implicitHeight + 24

    ColumnLayout {
        id: layout

        anchors.fill: parent
        anchors.leftMargin: Theme.padding
        anchors.rightMargin: Theme.padding
        anchors.topMargin: 12
        anchors.bottomMargin: 12
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 10

            Text {
                objectName: "osdIcon"
                Layout.preferredWidth: 18
                text: root.iconGlyph
                color: root.accentColor
                font.family: Theme.iconFont
                font.pixelSize: 15
                horizontalAlignment: Text.AlignHCenter
            }

            Text {
                objectName: "osdLabel"
                Layout.fillWidth: true
                text: root.labelText
                textFormat: Text.PlainText
                color: Theme.foreground
                font.family: Theme.font
                font.pixelSize: Theme.textBody
                elide: Text.ElideRight
            }

            Text {
                objectName: "osdValue"
                text: root.valueText
                textFormat: Text.PlainText
                color: root.muted ? Theme.mutedStrong : Theme.foreground
                font.family: Theme.font
                font.pixelSize: Theme.textBody
            }
        }

        Item {
            Layout.fillWidth: true
            implicitHeight: 8

            Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: Theme.border
            }

            Rectangle {
                objectName: "osdAmplifiedZone"
                visible: root.isVolume
                x: parent.width * 2 / 3
                width: parent.width / 3
                height: parent.height
                radius: height / 2
                color: Theme.purpleSoft
            }

            Rectangle {
                objectName: "osdFill"
                width: Math.round(root.fillFraction * parent.width)
                height: parent.height
                radius: height / 2
                color: root.accentColor

                Behavior on width {
                    NumberAnimation {
                        duration: Theme.animationFast
                        easing.type: Easing.OutCubic
                    }
                }

                Behavior on color {
                    ColorAnimation {
                        duration: Theme.animationFast
                    }
                }
            }

            Rectangle {
                objectName: "osdNominalMark"
                visible: root.isVolume
                x: Math.round(parent.width * 2 / 3 - width / 2)
                y: -3
                width: 2
                height: parent.height + 6
                color: Theme.cyan
            }
        }
    }
}
