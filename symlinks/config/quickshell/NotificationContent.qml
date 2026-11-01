pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

ColumnLayout {
    id: root

    required property var state
    spacing: Theme.spacing

    MenuHeader {
        Layout.fillWidth: true
        icon: "\uf0f3"
        title: "Notifications"
        subtitle: "Active and recent history"
        trailingItem: MenuIconButton {
            glyph: "\uf2f1"
            tooltip: "Refresh notifications"
            enabled: !root.state.loading
            onTriggered: root.state.refresh()
        }
    }

    Text {
        Layout.fillWidth: true
        visible: root.state.loading || !root.state.available
        text: root.state.error || "Loading notifications…"
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: root.state.error ? Theme.yellow : Theme.mutedStrong
        font.family: Theme.font
        font.pixelSize: Theme.textSmall
    }

    Repeater {
        model: [
            { title: "Active", entries: root.state.notifications },
            { title: "Recent history", entries: root.state.history }
        ]
        ColumnLayout {
            id: section
            required property var modelData
            Layout.fillWidth: true
            visible: root.state.available
            spacing: Theme.spacing

            Text {
                Layout.fillWidth: true
                text: section.modelData.title
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }
            Text {
                Layout.fillWidth: true
                visible: section.modelData.entries.length === 0
                text: "No notifications"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textBody
            }
            Repeater {
                model: section.modelData.entries
                Rectangle {
                    id: card
                    required property var modelData
                    Layout.fillWidth: true
                    implicitHeight: content.implicitHeight + 20
                    radius: Theme.itemRadius
                    color: Theme.raised
                    Accessible.name: modelData.app + ": " + modelData.summary
                    Accessible.description: modelData.body

                    ColumnLayout {
                        id: content
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.margins: 10
                        spacing: 4

                        Text {
                            Layout.fillWidth: true
                            text: card.modelData.app
                            textFormat: Text.PlainText
                            wrapMode: Text.WrapAnywhere
                            color: card.modelData.critical ? Theme.red : Theme.blue
                            font.family: Theme.font
                            font.pixelSize: Theme.textSmall
                        }
                        Text {
                            Layout.fillWidth: true
                            text: card.modelData.summary
                            textFormat: Text.PlainText
                            wrapMode: Text.WrapAnywhere
                            color: Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: Theme.textBody
                            font.bold: true
                        }
                        Text {
                            Layout.fillWidth: true
                            visible: text.length > 0
                            text: card.modelData.body
                            textFormat: Text.PlainText
                            wrapMode: Text.WrapAnywhere
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: Theme.textBody
                        }
                    }
                }
            }
        }
    }
}
