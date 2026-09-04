pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

ShellPopup {
    id: root

    required property var audio
    property bool devicesExpanded: false

    onVisibleChanged: {
        if (visible) {
            audio.refresh();
            outputSlider.forceActiveFocus();
        }
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.spacing

        MenuHeader {
            Layout.fillWidth: true
            title: "Volume"
            subtitle: root.audio.outputName || (root.audio.loading ? "Finding outputs..." : "No output device")
            icon: root.audio.muted ? "\uf6a9" : "\uf028"
            accentColor: root.audio.muted ? Theme.red : Theme.blue
            accentBackground: root.audio.muted ? Theme.redSoft : Theme.blueSoft
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: Theme.spacing
            spacing: Theme.spacing

            Text {
                Layout.fillWidth: true
                text: root.audio.available ? Math.round(outputSlider.value * 100) + "%" : "--"
                color: outputSlider.value > 1 ? Theme.yellow : Theme.foreground
                font.family: Theme.font
                font.pixelSize: 30
                font.weight: Font.DemiBold
                Accessible.name: root.audio.available ? "Volume " + text : "Volume unavailable"
            }

            Text {
                text: root.audio.muted ? "Muted" : outputSlider.value > 1 ? "Boost" : ""
                color: root.audio.muted ? Theme.red : Theme.yellow
                font.family: Theme.font
                font.pixelSize: Theme.textBody
            }

            MenuIconButton {
                enabled: root.audio.available && !root.audio.switchingOutput
                glyph: root.audio.muted ? "\uf6a9" : "\uf028"
                tooltip: root.audio.muted ? "Unmute output" : "Mute output"
                selected: root.audio.muted
                onTriggered: root.audio.toggleMuted()
            }
        }

        Slider {
            id: outputSlider

            Layout.fillWidth: true
            implicitHeight: 44
            from: 0
            to: 1.5
            stepSize: 0.01
            live: true
            focusPolicy: Qt.StrongFocus
            enabled: root.audio.available && !root.audio.switchingOutput
            opacity: enabled ? 1 : 0.45
            Accessible.name: "Output volume"
            Accessible.description: "Use arrow keys to adjust volume from 0 to 150 percent. Above 100 percent amplifies audio."
            onMoved: root.audio.setVolume(value)

            Binding {
                target: outputSlider
                property: "value"
                value: root.audio.volume
                when: !outputSlider.pressed
            }

            background: Item {
                x: outputSlider.leftPadding + outputSlider.handle.width / 2
                y: outputSlider.topPadding + outputSlider.availableHeight / 2 - height / 2
                width: outputSlider.availableWidth - outputSlider.handle.width
                height: 8

                Rectangle {
                    anchors.fill: parent
                    radius: height / 2
                    color: Theme.border
                }

                Rectangle {
                    x: parent.width * 2 / 3
                    width: parent.width / 3
                    height: parent.height
                    radius: height / 2
                    color: Theme.yellowSoft
                }

                Rectangle {
                    width: outputSlider.visualPosition * parent.width
                    height: parent.height
                    radius: height / 2
                    color: outputSlider.value > 1 ? Theme.yellow : Theme.blue
                }

                Rectangle {
                    x: parent.width * 2 / 3 - width / 2
                    y: -4
                    width: 2
                    height: parent.height + 8
                    color: Theme.yellow
                }
            }

            handle: Rectangle {
                x: outputSlider.leftPadding + outputSlider.visualPosition * (outputSlider.availableWidth - width)
                y: outputSlider.topPadding + outputSlider.availableHeight / 2 - height / 2
                implicitWidth: 22
                implicitHeight: 22
                radius: width / 2
                color: Theme.foreground
                border.width: outputSlider.activeFocus ? 4 : 3
                border.color: outputSlider.value > 1 ? Theme.yellow : Theme.blue

                Rectangle {
                    anchors.fill: parent
                    anchors.margins: -4
                    radius: width / 2
                    color: "transparent"
                    border.width: outputSlider.activeFocus ? 1 : 0
                    border.color: Theme.foreground
                }
            }
        }

        Item {
            Layout.fillWidth: true
            implicitHeight: scaleStart.implicitHeight

            Text {
                id: scaleStart

                text: "0%"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }

            Text {
                x: outputSlider.leftPadding + outputSlider.handle.width / 2 + (outputSlider.availableWidth - outputSlider.handle.width) * 2 / 3 - width / 2
                text: "100%"
                color: Theme.yellow
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }

            Text {
                anchors.right: parent.right
                text: "150%"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }
        }

        Text {
            Layout.fillWidth: true
            visible: root.audio.available && outputSlider.value > 1
            text: "Above 100% may distort audio."
            color: Theme.yellow
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
            wrapMode: Text.WordWrap
        }

        MenuButton {
            id: deviceToggle

            Layout.fillWidth: true
            Layout.topMargin: Theme.spacing
            label: "Output device"
            trailing: root.devicesExpanded ? "Hide" : "Choose"
            glyph: "\uf390"
            selected: root.devicesExpanded
            showChevron: false
            Accessible.description: root.devicesExpanded ? "Hide output devices" : "Choose the default output device"
            onTriggered: root.devicesExpanded = !root.devicesExpanded
        }

        ScrollView {
            id: deviceScroll

            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(deviceList.implicitHeight, Math.max(58, root.availableContentHeight - 310))
            visible: root.devicesExpanded && root.audio.outputs.length > 0
            clip: true
            contentWidth: availableWidth
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
            ScrollBar.vertical: ShellScrollBar {}

            ColumnLayout {
                id: deviceList

                width: deviceScroll.availableWidth
                spacing: 2

                Repeater {
                    model: root.audio.outputs

                    MenuButton {
                        id: deviceButton

                        required property var modelData

                        Layout.fillWidth: true
                        label: modelData.name
                        detail: modelData.available ? modelData.detail : "Disconnected"
                        trailing: selected ? "Active" : ""
                        selected: modelData.id === root.audio.outputId
                        enabled: modelData.available && !root.audio.switchingOutput
                        showChevron: false
                        Accessible.description: selected ? "Current default output" : "Make default output"
                        ShellToolTip {
                            text: deviceButton.modelData.name
                            visible: deviceButton.hovered || deviceButton.visualFocus
                        }
                        onTriggered: root.audio.selectOutput(modelData.id)
                        onActiveFocusChanged: {
                            if (!activeFocus)
                                return;
                            const viewport = deviceScroll.contentItem as Flickable;
                            if (!viewport)
                                return;
                            const top = deviceButton.y;
                            if (top < viewport.contentY)
                                viewport.contentY = top;
                            else if (top + height > viewport.contentY + viewport.height)
                                viewport.contentY = top + height - viewport.height;
                        }
                    }
                }
            }
        }

        Text {
            Layout.fillWidth: true
            visible: root.audio.error.length > 0 || root.audio.switchingOutput || (root.devicesExpanded && root.audio.outputs.length === 0)
            text: root.audio.error || (root.audio.switchingOutput ? "Switching output..." : root.audio.loading ? "Finding outputs..." : "No output devices connected.")
            color: root.audio.error ? Theme.red : Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
            wrapMode: Text.WordWrap
            Accessible.role: Accessible.StaticText
        }

        MenuButton {
            Layout.fillWidth: true
            visible: root.audio.error.length > 0
            label: root.audio.loading ? "Retrying..." : "Retry"
            glyph: "\uf2f1"
            enabled: !root.audio.loading
            showChevron: false
            onTriggered: root.audio.refresh(true)
        }
    }
}
