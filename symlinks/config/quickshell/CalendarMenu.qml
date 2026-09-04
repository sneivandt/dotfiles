pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

ShellPopup {
    id: root

    property int monthOffset: 0
    property date shownMonth: new Date(clock.date.getFullYear(), clock.date.getMonth() + monthOffset, 1)
    readonly property int firstWeekday: (shownMonth.getDay() + 6) % 7

    function dateForCell(index) {
        return new Date(shownMonth.getFullYear(), shownMonth.getMonth(), index - firstWeekday + 1);
    }

    function isToday(date) {
        return date.getDate() === clock.date.getDate() && date.getMonth() === clock.date.getMonth() && date.getFullYear() === clock.date.getFullYear();
    }

    onVisibleChanged: {
        if (!visible)
            monthOffset = 0;
    }

    SystemClock {
        id: clock

        precision: SystemClock.Minutes
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.padding

        MenuHeader {
            id: header

            Layout.fillWidth: true
            icon: "\uf017"
            title: Qt.formatTime(clock.date, "HH:mm")
            subtitle: Qt.formatDate(clock.date, "dddd, MMMM d")
            accentColor: Theme.mutedStrong
            accentBackground: "transparent"
        }

        Flickable {
            id: scroll

            function reveal(item) {
                const position = item.mapToItem(calendarContent, 0, 0);
                if (position.y < contentY)
                    contentY = position.y;
                else if (position.y + item.height > contentY + height)
                    contentY = Math.max(0, position.y + item.height - height);
            }

            Layout.fillWidth: true
            implicitHeight: Math.min(contentHeight, Math.max(0, root.availableContentHeight - header.implicitHeight - Theme.padding))
            contentHeight: calendarContent.implicitHeight
            contentWidth: width
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.VerticalFlick
            ScrollBar.vertical: ShellScrollBar {}

            ColumnLayout {
                id: calendarContent

                width: scroll.width
                spacing: Theme.spacing

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacing

                    MenuIconButton {
                        id: previousMonth

                        glyph: "\uf104"
                        tooltip: "Previous month"
                        onTriggered: root.monthOffset--
                        onActiveFocusChanged: {
                            if (activeFocus)
                                scroll.reveal(previousMonth);
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: Qt.formatDate(root.shownMonth, "MMMM yyyy")
                        color: Theme.foreground
                        font.family: Theme.font
                        font.pixelSize: Theme.textHeading
                        font.weight: Font.DemiBold
                        horizontalAlignment: Text.AlignHCenter
                    }

                    MenuIconButton {
                        id: nextMonth

                        glyph: "\uf105"
                        tooltip: "Next month"
                        onTriggered: root.monthOffset++
                        onActiveFocusChanged: {
                            if (activeFocus)
                                scroll.reveal(nextMonth);
                        }
                    }
                }

                GridLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: Theme.spacing
                    Layout.bottomMargin: Theme.spacing
                    columns: 7
                    columnSpacing: 4
                    rowSpacing: 4

                    Repeater {
                        model: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

                        Text {
                            required property string modelData

                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            Layout.preferredHeight: 24
                            text: modelData
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: Theme.textSmall
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                    }

                    Repeater {
                        model: 42

                        AbstractButton {
                            id: day

                            required property int index
                            readonly property date cellDate: root.dateForCell(index)
                            readonly property bool inMonth: cellDate.getMonth() === root.shownMonth.getMonth()
                            readonly property bool today: root.isToday(cellDate)

                            function changeMonth() {
                                const previous = cellDate < root.shownMonth;
                                (previous ? previousMonth : nextMonth).forceActiveFocus(Qt.TabFocusReason);
                                root.monthOffset += previous ? -1 : 1;
                            }

                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            Layout.preferredHeight: 40
                            enabled: !inMonth
                            hoverEnabled: enabled
                            focusPolicy: enabled ? Qt.StrongFocus : Qt.NoFocus
                            padding: 0
                            Accessible.name: Qt.formatDate(cellDate, "dddd, MMMM d, yyyy") + (today ? ", today" : "")
                            Accessible.description: inMonth ? "" : "Show " + Qt.formatDate(cellDate, "MMMM yyyy")
                            onClicked: changeMonth()
                            Keys.onReturnPressed: event => {
                                if (!event.isAutoRepeat)
                                    changeMonth();
                            }
                            Keys.onEnterPressed: event => {
                                if (!event.isAutoRepeat)
                                    changeMonth();
                            }
                            onActiveFocusChanged: {
                                if (activeFocus)
                                    scroll.reveal(day);
                            }

                            contentItem: Text {
                                text: day.cellDate.getDate()
                                color: day.today ? Theme.blue : (day.inMonth ? Theme.foreground : Theme.mutedStrong)
                                font.family: Theme.font
                                font.pixelSize: Theme.textBody
                                font.weight: day.today ? Font.DemiBold : Font.Normal
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }

                            background: Rectangle {
                                radius: Theme.controlRadius
                                color: day.today ? Theme.blueSoft : (day.down ? Theme.pressed : (day.hovered ? Theme.hover : "transparent"))
                                border.width: day.visualFocus ? 1 : 0
                                border.color: Theme.blue
                            }

                            HoverHandler {
                                enabled: day.enabled
                                cursorShape: Qt.PointingHandCursor
                            }
                        }
                    }
                }

                MenuButton {
                    id: todayButton

                    Layout.fillWidth: true
                    glyph: "\uf133"
                    label: root.monthOffset === 0 ? "Today" : "Back to today"
                    trailing: Qt.formatDate(clock.date, "MMM d")
                    clickable: root.monthOffset !== 0
                    showChevron: false
                    onTriggered: {
                        previousMonth.forceActiveFocus(Qt.TabFocusReason);
                        root.monthOffset = 0;
                    }
                    onActiveFocusChanged: {
                        if (activeFocus)
                            scroll.reveal(todayButton);
                    }
                }
            }
        }
    }
}
