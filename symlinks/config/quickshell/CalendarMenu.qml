import QtQuick
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property int monthOffset: 0
    property date shownMonth: new Date(clock.date.getFullYear(), clock.date.getMonth() + monthOffset, 1)
    readonly property int firstWeekday: (shownMonth.getDay() + 6) % 7

    function dateForCell(index) {
        return new Date(shownMonth.getFullYear(), shownMonth.getMonth(), index - firstWeekday + 1);
    }

    function isToday(date) {
        return date.getDate() === clock.date.getDate() && date.getMonth() === clock.date.getMonth() && date.getFullYear() === clock.date.getFullYear();
    }

    implicitWidth: 376
    implicitHeight: 430
    color: "transparent"
    grabFocus: true
    onVisibleChanged: {
        if (!visible)
            monthOffset = 0;
    }

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

    SystemClock {
        id: clock

        precision: SystemClock.Minutes
    }

    MenuPanel {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 8

            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 54

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 2
                    anchors.rightMargin: 2
                    spacing: 10

                    Rectangle {
                        implicitWidth: 34
                        implicitHeight: 34
                        radius: Theme.controlRadius
                        color: Theme.cyanSoft

                        Text {
                            anchors.centerIn: parent
                            text: "\uf017"
                            color: Theme.cyan
                            font.family: Theme.iconFont
                            font.pixelSize: 14
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        Text {
                            text: Qt.formatTime(clock.date, "HH:mm")
                            color: Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: 25
                            font.weight: Font.DemiBold
                        }

                        Text {
                            text: Qt.formatDate(clock.date, "dddd, MMMM d")
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: 10
                        }
                    }

                    ColumnLayout {
                        spacing: 1

                        Text {
                            Layout.alignment: Qt.AlignRight
                            text: "LOCAL"
                            color: Theme.cyan
                            font.family: Theme.font
                            font.pixelSize: 9
                            font.weight: Font.DemiBold
                        }

                        Text {
                            Layout.alignment: Qt.AlignRight
                            text: Qt.formatDateTime(clock.date, "t")
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: 9
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 294
                radius: Theme.itemRadius
                color: Theme.raised
                border.width: 1
                border.color: Theme.borderSubtle

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 6
                    spacing: 6

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.leftMargin: 2
                        Layout.rightMargin: 2
                        Layout.preferredHeight: 34

                        MenuIconButton {
                            icon: "\uf104"
                            accentColor: Theme.cyan
                            accentBackground: Theme.cyanSoft
                            onTriggered: root.monthOffset--
                        }

                        Text {
                            Layout.fillWidth: true
                            text: Qt.formatDate(root.shownMonth, "MMMM yyyy")
                            color: Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: 14
                            font.weight: Font.DemiBold
                            horizontalAlignment: Text.AlignHCenter
                        }

                        MenuIconButton {
                            icon: "\uf105"
                            accentColor: Theme.cyan
                            accentBackground: Theme.cyanSoft
                            onTriggered: root.monthOffset++
                        }
                    }

                    GridLayout {
                        columns: 7
                        columnSpacing: 3
                        rowSpacing: 3
                        Layout.fillWidth: true

                        Repeater {
                            model: ["M", "T", "W", "T", "F", "S", "S"]

                            Text {
                                required property string modelData
                                required property int index

                                text: modelData
                                color: index >= 5 ? Theme.muted : Theme.mutedStrong
                                font.family: Theme.font
                                font.pixelSize: 9
                                font.weight: Font.DemiBold
                                horizontalAlignment: Text.AlignHCenter
                                Layout.preferredWidth: 45
                                Layout.preferredHeight: 20
                            }
                        }

                        Repeater {
                            model: 42

                            Rectangle {
                                required property int index
                                readonly property date cellDate: root.dateForCell(index)
                                readonly property bool inMonth: cellDate.getMonth() === root.shownMonth.getMonth()
                                readonly property bool today: root.isToday(cellDate)
                                readonly property bool adjacentMonth: !inMonth

                                Layout.preferredWidth: 45
                                Layout.preferredHeight: 34
                                radius: Theme.controlRadius
                                color: today ? Theme.blue : (dayMouse.pressed ? Theme.pressed : (dayMouse.containsMouse ? Theme.hover : "transparent"))
                                border.width: today || dayMouse.containsMouse ? 1 : 0
                                border.color: today ? Theme.blue : Theme.borderSubtle

                                Text {
                                    anchors.centerIn: parent
                                    text: parent.cellDate.getDate()
                                    color: parent.today ? Theme.backgroundSolid : (parent.adjacentMonth ? Theme.muted : Theme.foreground)
                                    font.family: Theme.font
                                    font.pixelSize: 12
                                    font.weight: parent.today ? Font.DemiBold : Font.Normal
                                }

                                MouseArea {
                                    id: dayMouse

                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: parent.adjacentMonth ? Qt.PointingHandCursor : Qt.ArrowCursor
                                    onClicked: {
                                        if (parent.cellDate < root.shownMonth)
                                            root.monthOffset--;
                                        else if (!parent.inMonth)
                                            root.monthOffset++;
                                    }
                                    onWheel: (event) => {
                                        if (event.angleDelta.y > 0)
                                            root.monthOffset--;
                                        else if (event.angleDelta.y < 0)
                                            root.monthOffset++;
                                    }
                                }

                                Behavior on color {
                                    ColorAnimation {
                                        duration: Theme.animationFast
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Rectangle {
                id: todayFooter

                Layout.fillWidth: true
                Layout.preferredHeight: 38
                radius: Theme.controlRadius
                color: footerMouse.pressed ? Theme.pressed : (footerMouse.containsMouse ? Theme.hover : "transparent")
                border.width: 1
                border.color: footerMouse.containsMouse ? Theme.cyanSoft : Theme.borderSubtle

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 10
                    anchors.rightMargin: 8
                    spacing: 8

                    Text {
                        text: "\uf133"
                        color: Theme.cyan
                        font.family: Theme.iconFont
                        font.pixelSize: 12
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.monthOffset === 0 ? "Today" : "Back to today"
                        color: Theme.foreground
                        font.family: Theme.font
                        font.pixelSize: 11
                        font.weight: Font.DemiBold
                    }

                    Rectangle {
                        implicitWidth: footerDate.implicitWidth + 14
                        implicitHeight: 24
                        radius: 7
                        color: Theme.cyanSoft

                        Text {
                            id: footerDate

                            anchors.centerIn: parent
                            text: Qt.formatDate(clock.date, "MMM d")
                            color: Theme.cyan
                            font.family: Theme.font
                            font.pixelSize: 9
                            font.weight: Font.DemiBold
                        }
                    }

                }

                MouseArea {
                    id: footerMouse

                    anchors.fill: parent
                    enabled: root.monthOffset !== 0
                    hoverEnabled: true
                    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onClicked: root.monthOffset = 0
                }

                Behavior on color {
                    ColorAnimation {
                        duration: Theme.animationFast
                    }
                }

                Behavior on border.color {
                    ColorAnimation {
                        duration: Theme.animationFast
                    }
                }
            }
        }
    }
}
