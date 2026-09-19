pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

ColumnLayout {
    id: root

    required property date currentDate
    property real availableContentHeight: 600
    property int monthOffset: 0
    readonly property date referenceDate: currentDate
    readonly property date shownMonth: new Date(referenceDate.getFullYear(), referenceDate.getMonth() + monthOffset, 1, 12)
    readonly property int firstWeekday: (shownMonth.getDay() + 6) % 7

    function utcTime() {
        return String(currentDate.getUTCHours()).padStart(2, "0") + ":" + String(currentDate.getUTCMinutes()).padStart(2, "0");
    }

    function dateForCell(index) {
        return new Date(shownMonth.getFullYear(), shownMonth.getMonth(), index - firstWeekday + 1, 12);
    }

    function isToday(date) {
        return date.getDate() === referenceDate.getDate() && date.getMonth() === referenceDate.getMonth() && date.getFullYear() === referenceDate.getFullYear();
    }

    spacing: Theme.spacing

    RowLayout {
        id: clockRow

        Layout.fillWidth: true
        spacing: Theme.spacing
        Accessible.role: Accessible.StaticText
        Accessible.name: Qt.formatDate(root.currentDate, "dddd, MMMM d, yyyy") + ", " + Qt.formatTime(root.currentDate, "HH:mm") + ", UTC " + root.utcTime()

        Text {
            objectName: "localClock"
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Layout.alignment: Qt.AlignVCenter
            text: Qt.formatTime(root.currentDate, "HH:mm")
            color: Theme.foreground
            font.family: Theme.font
            font.pixelSize: 24
            font.weight: Font.DemiBold
        }

        Text {
            objectName: "utcClock"
            Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
            text: root.utcTime() + " UTC"
            color: Theme.cyan
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
        }
    }

    Flickable {
        id: scroll
        objectName: "calendarScroll"

        function reveal(item) {
            const position = item.mapToItem(calendarContent, 0, 0);
            if (position.y < contentY)
                contentY = position.y;
            else if (position.y + item.height > contentY + height)
                contentY = Math.max(0, position.y + item.height - height);
        }

        Layout.fillWidth: true
        implicitHeight: Math.min(contentHeight, Math.max(0, root.availableContentHeight - clockRow.implicitHeight - Theme.spacing))
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
                    objectName: "previousMonth"

                    glyph: "\uf104"
                    tooltip: "Previous month"
                    onTriggered: root.monthOffset--
                    onActiveFocusChanged: {
                        if (activeFocus)
                            scroll.reveal(previousMonth);
                    }
                }

                Text {
                    objectName: "shownMonth"
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    text: Qt.formatDate(root.shownMonth, "MMMM yyyy")
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: Theme.textHeading
                    font.weight: Font.DemiBold
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                }

                MenuIconButton {
                    id: nextMonth
                    objectName: "nextMonth"

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
                columns: 7
                columnSpacing: 2
                rowSpacing: 2

                Repeater {
                    model: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

                    Text {
                        required property string modelData

                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        Layout.preferredHeight: 20
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
                        objectName: "day" + index

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
                        Layout.preferredHeight: 32
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
                            color: day.today ? Theme.backgroundSolid : (day.inMonth ? Theme.foreground : Theme.mutedStrong)
                            font.family: Theme.font
                            font.pixelSize: Theme.textBody
                            font.weight: day.today ? Font.DemiBold : Font.Normal
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            radius: Theme.controlRadius
                            color: day.today ? Theme.blue : (day.down ? Theme.pressed : (day.hovered ? Theme.hover : "transparent"))
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
                objectName: "todayButton"

                Layout.fillWidth: true
                visible: root.monthOffset !== 0
                implicitHeight: 32
                padding: 6
                glyph: "\uf133"
                label: "Back to today"
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
