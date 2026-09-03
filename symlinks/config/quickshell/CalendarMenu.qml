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

    implicitWidth: 346
    implicitHeight: 384
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
            spacing: 10

            MenuHeader {
                Layout.fillWidth: true
                Layout.leftMargin: 2
                Layout.rightMargin: 2
                icon: "\uf133"
                title: Qt.formatDate(root.shownMonth, "MMMM yyyy")
                subtitle: root.monthOffset === 0 ? Qt.formatDate(clock.date, "dddd, MMMM d") : "Browsing calendar"
                accentColor: Theme.cyan
                accentBackground: Theme.cyanSoft
                trailingItem: Row {
                    spacing: 4

                    MenuIconButton {
                        icon: "\uf104"
                        accentColor: Theme.cyan
                        accentBackground: Theme.cyanSoft
                        onTriggered: root.monthOffset--
                    }

                    MenuIconButton {
                        icon: "\uf105"
                        accentColor: Theme.cyan
                        accentBackground: Theme.cyanSoft
                        onTriggered: root.monthOffset++
                    }
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
                        color: index >= 5 ? Theme.mutedStrong : Theme.cyan
                        font.family: Theme.font
                        font.pixelSize: 9
                        font.weight: Font.DemiBold
                        horizontalAlignment: Text.AlignHCenter
                        Layout.preferredWidth: 42
                        Layout.preferredHeight: 22
                    }
                }

                Repeater {
                    model: 42

                    Rectangle {
                        required property int index
                        readonly property int day: index - root.firstWeekday + 1
                        readonly property bool valid: day >= 1 && day <= root.daysInMonth
                        readonly property bool today: valid && root.isToday(day)

                        Layout.preferredWidth: 42
                        Layout.preferredHeight: 36
                        radius: Theme.controlRadius
                        color: today ? Theme.blue : (dayMouse.pressed && valid ? Theme.pressed : (dayMouse.containsMouse && valid ? Theme.hover : "transparent"))
                        border.width: valid && (today || dayMouse.containsMouse) ? 1 : 0
                        border.color: today ? Theme.blue : Theme.borderSubtle

                        Text {
                            anchors.centerIn: parent
                            text: parent.valid ? parent.day : ""
                            color: parent.today ? Theme.backgroundSolid : Theme.foreground
                            font.family: Theme.font
                            font.pixelSize: 12
                            font.weight: parent.today ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            id: dayMouse

                            anchors.fill: parent
                            enabled: parent.valid
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                        }

                        Behavior on color {
                            ColorAnimation {
                                duration: Theme.animationFast
                            }
                        }
                    }
                }
            }

            MenuButton {
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                icon: "\uf073"
                label: root.monthOffset === 0 ? "Today" : "Back to today"
                trailing: Qt.formatDate(clock.date, "MMM d")
                showChevron: false
                onTriggered: root.monthOffset = 0
            }
        }
    }
}
