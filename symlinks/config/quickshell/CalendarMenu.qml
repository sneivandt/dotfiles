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
    readonly property int daysInMonth: new Date(shownMonth.getFullYear(), shownMonth.getMonth() + 1, 0).getDate()

    function isToday(day) {
        return day === clock.date.getDate() && shownMonth.getMonth() === clock.date.getMonth() && shownMonth.getFullYear() === clock.date.getFullYear();
    }

    implicitWidth: 322
    implicitHeight: 326
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
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 6);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    SystemClock {
        id: clock

        precision: SystemClock.Minutes
    }

    Rectangle {
        anchors.fill: parent
        radius: 10
        color: Theme.backgroundSolid
        border.width: 1
        border.color: Theme.border

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 8

            RowLayout {
                Layout.fillWidth: true

                Rectangle {
                    implicitWidth: 30
                    implicitHeight: 28
                    radius: 6
                    color: previousMouse.containsMouse ? Theme.hover : "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: "\uf104"
                        color: Theme.blue
                        font.family: Theme.iconFont
                        font.pixelSize: 13
                    }

                    MouseArea {
                        id: previousMouse

                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.monthOffset--
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: Qt.formatDate(root.shownMonth, "MMMM yyyy")
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: 16
                    font.weight: Font.DemiBold
                    horizontalAlignment: Text.AlignHCenter
                }

                Rectangle {
                    implicitWidth: 30
                    implicitHeight: 28
                    radius: 6
                    color: nextMouse.containsMouse ? Theme.hover : "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: "\uf105"
                        color: Theme.blue
                        font.family: Theme.iconFont
                        font.pixelSize: 13
                    }

                    MouseArea {
                        id: nextMouse

                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.monthOffset++
                    }
                }
            }

            GridLayout {
                columns: 7
                columnSpacing: 2
                rowSpacing: 2
                Layout.fillWidth: true

                Repeater {
                    model: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

                    Text {
                        required property string modelData

                        text: modelData
                        color: Theme.blue
                        font.family: Theme.font
                        font.pixelSize: 10
                        font.weight: Font.DemiBold
                        horizontalAlignment: Text.AlignHCenter
                        Layout.preferredWidth: 40
                        Layout.preferredHeight: 24
                    }
                }

                Repeater {
                    model: 42

                    Rectangle {
                        required property int index
                        readonly property int day: index - root.firstWeekday + 1
                        readonly property bool valid: day >= 1 && day <= root.daysInMonth
                        readonly property bool today: valid && root.isToday(day)

                        Layout.preferredWidth: 40
                        Layout.preferredHeight: 34
                        radius: 7
                        color: today ? Theme.red : (dayMouse.containsMouse && valid ? Theme.hover : "transparent")

                        Text {
                            anchors.centerIn: parent
                            text: parent.valid ? parent.day : ""
                            color: parent.today ? Theme.backgroundSolid : Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: 12
                            font.weight: parent.today ? Font.Bold : Font.Normal
                        }

                        MouseArea {
                            id: dayMouse

                            anchors.fill: parent
                            enabled: parent.valid
                            hoverEnabled: true
                        }
                    }
                }
            }

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                implicitWidth: todayText.implicitWidth + 18
                implicitHeight: 27
                radius: 6
                color: todayMouse.containsMouse ? Theme.hover : Theme.raised

                Text {
                    id: todayText

                    anchors.centerIn: parent
                    text: "Today"
                    color: Theme.cyan
                    font.family: Theme.font
                    font.pixelSize: 11
                }

                MouseArea {
                    id: todayMouse

                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.monthOffset = 0
                }
            }
        }
    }
}
