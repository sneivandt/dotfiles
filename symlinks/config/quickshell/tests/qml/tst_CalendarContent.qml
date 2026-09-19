import QtQuick
import QtTest
import "../.." as Shell

Item {
    width: 304
    height: 480

    Component {
        id: calendarComponent

        Shell.CalendarContent {
            currentDate: new Date(2026, 8, 19, 14, 32)
            availableContentHeight: 480
        }
    }

    TestCase {
        name: "CalendarContent"
        when: windowShown

        function test_clocks_are_read_only_status_text() {
            const calendar = createTemporaryObject(calendarComponent, parent);
            verify(calendar !== null);

            const localClock = findChild(calendar, "localClock");
            const utcClock = findChild(calendar, "utcClock");
            verify(localClock !== null);
            verify(utcClock !== null);
            compare(localClock.text, Qt.formatTime(calendar.currentDate, "HH:mm"));
            compare(utcClock.text, calendar.utcTime() + " UTC");
            compare(findChild(calendar, "localTimezone"), null);
            compare(findChild(calendar, "utcTimezone"), null);
        }
    }
}
