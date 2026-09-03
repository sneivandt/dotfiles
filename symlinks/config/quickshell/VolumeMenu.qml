import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    required property var audio

    implicitWidth: 350
    implicitHeight: 148
    color: "transparent"
    grabFocus: true

    anchor {
        window: root.anchorItem.QsWindow.window
        adjustment: PopupAdjustment.SlideX | PopupAdjustment.FlipY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            const content = root.anchorItem.QsWindow.contentItem;
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 6);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: 10
        color: Theme.backgroundSolid
        border.width: 1
        border.color: Theme.border

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 4
                Layout.rightMargin: 4

                Text {
                    text: "Volume"
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: 16
                    font.weight: Font.DemiBold
                }

                Item {
                    Layout.fillWidth: true
                }

                Text {
                    text: root.audio.available ? Math.round(root.audio.volume * 100) + "%" : "Unavailable"
                    color: Theme.muted
                    font.family: Theme.font
                    font.pixelSize: 11
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 88
                radius: 8
                color: Theme.raised
                opacity: root.audio.available ? 1 : 0.55

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 5

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Text {
                            Layout.fillWidth: true
                            text: root.audio.available ? "Default output" : "No output device"
                            color: Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: 11
                            elide: Text.ElideRight
                        }

                        Rectangle {
                            implicitWidth: 28
                            implicitHeight: 24
                            radius: 6
                            color: outputMute.containsMouse ? Theme.hover : Theme.backgroundSolid

                            Text {
                                anchors.centerIn: parent
                                text: root.audio.muted ? "\uf6a9" : "\uf028"
                                color: root.audio.muted ? Theme.red : Theme.blue
                                font.family: Theme.iconFont
                                font.pixelSize: 12
                            }

                            MouseArea {
                                id: outputMute

                                anchors.fill: parent
                                enabled: root.audio.available
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.audio.toggleMuted()
                            }
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
                            height: 5
                            radius: 3
                            color: Theme.backgroundSolid

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
                            implicitWidth: 15
                            implicitHeight: 15
                            radius: 8
                            color: Theme.foreground
                            border.width: 2
                            border.color: outputSlider.value > 1 ? Theme.yellow : Theme.blue
                        }
                    }
                }
            }
        }
    }
}
