import QtQuick

ShellPopup {
    id: root

    required property date currentDate
    panelWidth: 304
    alignToWindowFrame: true

    onVisibleChanged: {
        if (!visible)
            calendar.monthOffset = 0;
    }

    CalendarContent {
        id: calendar

        width: parent.width
        currentDate: root.currentDate
        availableContentHeight: root.availableContentHeight
    }
}
