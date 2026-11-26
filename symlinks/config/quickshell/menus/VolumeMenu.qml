pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"
import "../lib/Theme.js" as Theme

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
            id: header

            Layout.fillWidth: true
            title: "Volume"
            subtitle: root.audio.outputName || (root.audio.loading ? "Finding outputs..." : "No output device")
            icon: root.audio.muted ? "\uf6a9" : "\uf028"
            accentColor: root.audio.muted ? Theme.red : Theme.blue
            accentBackground: root.audio.muted ? Theme.redSoft : Theme.blueSoft
        }

        RowLayout {
            id: readoutRow

            Layout.fillWidth: true
            spacing: Theme.spacing

            Text {
                objectName: "volumePercentLabel"
                Layout.fillWidth: true
                text: root.audio.available ? Math.round(outputSlider.value * 100) + "%" : "--"
                color: !root.audio.available || root.audio.muted ? Theme.mutedStrong : (outputSlider.value > 1 ? Theme.purple : Theme.foreground)
                font.family: Theme.font
                font.pixelSize: 30
                font.weight: Font.DemiBold
                Accessible.name: root.audio.available ? "Volume " + text : "Volume unavailable"
            }

            Text {
                text: root.audio.muted ? "Muted" : outputSlider.value > 1 ? "Boost" : ""
                color: root.audio.muted ? Theme.red : Theme.purple
                font.family: Theme.font
                font.pixelSize: Theme.textBody
            }

            MenuIconButton {
                enabled: root.audio.available && !root.audio.switchingOutput
                glyph: root.audio.muted ? "\uf6a9" : "\uf028"
                tooltip: root.audio.muted ? "Unmute output" : "Mute output"
                selected: root.audio.muted
                accentColor: root.audio.muted ? Theme.red : Theme.blue
                accentBackground: root.audio.muted ? Theme.redSoft : Theme.blueSoft
                onTriggered: root.audio.toggleMuted()
            }
        }

        VolumeSlider {
            id: outputSlider
            objectName: "volumeSlider"

            Layout.fillWidth: true
            muted: root.audio.muted
            enabled: root.audio.available && !root.audio.switchingOutput
            onMoved: root.audio.setVolume(outputSlider.snapDetent())

            Binding {
                target: outputSlider
                property: "value"
                value: root.audio.volume
                when: !outputSlider.pressed
            }
        }

        Item {
            id: scaleRow

            Layout.fillWidth: true
            implicitHeight: nominalButton.implicitHeight

            Text {
                id: scaleStart

                anchors.verticalCenter: parent.verticalCenter
                text: "0%"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }

            AbstractButton {
                id: nominalButton
                objectName: "volumeNominalButton"

                x: outputSlider.leftPadding + outputSlider.handle.width / 2 + (outputSlider.availableWidth - outputSlider.handle.width) * 2 / 3 - width / 2
                padding: 2
                hoverEnabled: true
                enabled: root.audio.available && !root.audio.switchingOutput
                focusPolicy: Qt.StrongFocus
                Accessible.name: "Set volume to 100 percent"
                onClicked: root.audio.setVolume(1.0)
                Keys.onReturnPressed: root.audio.setVolume(1.0)
                Keys.onEnterPressed: root.audio.setVolume(1.0)

                HoverHandler {
                    cursorShape: nominalButton.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                }

                background: Rectangle {
                    radius: 4
                    color: nominalButton.down ? Theme.pressed : (nominalButton.hovered ? Theme.hover : "transparent")
                    border.width: nominalButton.visualFocus ? 1 : 0
                    border.color: Theme.cyan
                }

                contentItem: Text {
                    text: "100%"
                    color: Theme.cyan
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    font.weight: nominalButton.hovered ? Font.DemiBold : Font.Normal
                }
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: "150%"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }
        }

        MenuBanner {
            id: boostBanner

            Layout.fillWidth: true
            visible: root.audio.available && !root.audio.muted && outputSlider.value > 1
            tone: "warning"
            text: "Above 100% may distort audio."
        }

        MenuButton {
            id: deviceToggle

            Layout.fillWidth: true
            Layout.topMargin: 4
            label: "Output device"
            trailing: root.devicesExpanded ? "Hide" : "Choose"
            glyph: "\uf390"
            selected: root.devicesExpanded
            showChevron: false
            Accessible.description: root.devicesExpanded ? "Hide output devices" : "Choose the default output device"
            onTriggered: root.devicesExpanded = !root.devicesExpanded

            trailingItem: Text {
                text: "\uf107"
                color: root.devicesExpanded || deviceToggle.hovered || deviceToggle.visualFocus ? Theme.blue : Theme.mutedStrong
                font.family: Theme.iconFont
                font.pixelSize: Theme.textSmall
                rotation: root.devicesExpanded ? 180 : 0

                Behavior on rotation {
                    NumberAnimation {
                        duration: Theme.animationFast
                        easing.type: Easing.OutCubic
                    }
                }
            }
        }

        ScrollView {
            id: deviceScroll

            readonly property real fixedChromeHeight: header.implicitHeight + readoutRow.implicitHeight + outputSlider.implicitHeight + scaleRow.implicitHeight + deviceToggle.implicitHeight + (boostBanner.visible ? boostBanner.implicitHeight + Theme.spacing : 0) + (statusBanner.visible ? statusBanner.implicitHeight + Theme.spacing : 0) + (dismissErrorButton.visible ? dismissErrorButton.implicitHeight + Theme.spacing : 0) + Theme.spacing * 5 + 4

            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(deviceList.implicitHeight, Math.max(58, root.availableContentHeight - fixedChromeHeight))
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
                            visible: deviceButton.labelTruncated && (deviceButton.hovered || deviceButton.visualFocus)
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

        MenuBanner {
            id: statusBanner

            Layout.fillWidth: true
            visible: root.audio.error.length > 0 || root.audio.switchingOutput || (root.devicesExpanded && root.audio.outputs.length === 0)
            tone: root.audio.error.length > 0 ? "error" : "info"
            spinning: root.audio.error.length === 0 && (root.audio.switchingOutput || root.audio.loading)
            text: root.audio.error || (root.audio.switchingOutput ? "Switching output..." : root.audio.loading ? "Finding outputs..." : "No output devices connected.")
        }

        MenuButton {
            id: dismissErrorButton
            objectName: "volumeDismissErrorButton"
            Layout.fillWidth: true
            visible: root.audio.actionError.length > 0
            label: "Dismiss"
            glyph: "\uf00d"
            showChevron: false
            onTriggered: root.audio.clearError()
        }
    }
}
