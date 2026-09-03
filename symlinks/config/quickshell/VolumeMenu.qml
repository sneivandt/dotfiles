import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    required property var audio

    implicitWidth: 370
    implicitHeight: 184
    color: "transparent"
    grabFocus: true

    anchor {
        window: root.anchorItem.QsWindow.window
        adjustment: PopupAdjustment.SlideX | PopupAdjustment.FlipY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            const content = root.anchorItem.QsWindow.contentItem;
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 8);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    MenuPanel {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 10

            MenuHeader {
                Layout.fillWidth: true
                Layout.leftMargin: 2
                Layout.rightMargin: 2
                icon: root.audio.muted ? "\uf6a9" : "\uf028"
                title: "Volume"
                subtitle: root.audio.available ? "Default output" : "No output device"
                accentColor: root.audio.muted ? Theme.red : Theme.blue
                accentBackground: root.audio.muted ? Theme.redSoft : Theme.blueSoft
                trailingItem: Rectangle {
                    implicitWidth: volumeText.implicitWidth + 18
                    implicitHeight: 28
                    radius: Theme.controlRadius
                    color: Theme.raised
                    border.width: 1
                    border.color: Theme.borderSubtle

                    Text {
                        id: volumeText

                        anchors.centerIn: parent
                        text: root.audio.available ? Math.round(root.audio.volume * 100) + "%" : "--"
                        color: root.audio.volume > 1 ? Theme.yellow : Theme.foreground
                        font.family: Theme.font
                        font.pixelSize: 11
                        font.weight: Font.DemiBold
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 108
                radius: Theme.itemRadius
                color: Theme.raised
                border.width: 1
                border.color: Theme.borderSubtle
                opacity: root.audio.available ? 1 : 0.55

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 8

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1

                            Text {
                                Layout.fillWidth: true
                                text: "Output level"
                                color: Theme.foreground
                                font.family: Theme.font
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.audio.muted ? "Audio is muted" : (root.audio.volume > 1 ? "Amplified above 100%" : "System audio")
                                color: root.audio.muted ? Theme.red : (root.audio.volume > 1 ? Theme.yellow : Theme.mutedStrong)
                                font.family: Theme.font
                                font.pixelSize: 10
                            }
                        }

                        MenuIconButton {
                            enabled: root.audio.available
                            icon: root.audio.muted ? "\uf6a9" : "\uf028"
                            accentColor: root.audio.muted ? Theme.red : Theme.blue
                            accentBackground: root.audio.muted ? Theme.redSoft : Theme.blueSoft
                            selected: root.audio.muted
                            onTriggered: root.audio.toggleMuted()
                        }
                    }

                    Slider {
                        id: outputSlider

                        Layout.fillWidth: true
                        from: 0
                        to: 1.5
                        stepSize: 0.01
                        enabled: root.audio.available
                        onMoved: root.audio.setVolume(value)

                        Binding {
                            target: outputSlider
                            property: "value"
                            value: root.audio.volume
                            when: !outputSlider.pressed
                        }

                        background: Rectangle {
                            x: outputSlider.leftPadding
                            y: outputSlider.topPadding + outputSlider.availableHeight / 2 - height / 2
                            width: outputSlider.availableWidth
                            height: 6
                            radius: 3
                            color: Theme.borderSubtle

                            Rectangle {
                                width: outputSlider.visualPosition * parent.width
                                height: parent.height
                                radius: 3
                                color: outputSlider.value > 1 ? Theme.yellow : Theme.blue
                            }
                        }

                        handle: Rectangle {
                            x: outputSlider.leftPadding + outputSlider.visualPosition * (outputSlider.availableWidth - width)
                            y: outputSlider.topPadding + outputSlider.availableHeight / 2 - height / 2
                            implicitWidth: 16
                            implicitHeight: 16
                            radius: 8
                            color: Theme.foreground
                            border.width: 3
                            border.color: outputSlider.value > 1 ? Theme.yellow : Theme.blue
                        }
                    }
                }
            }
        }
    }
}
