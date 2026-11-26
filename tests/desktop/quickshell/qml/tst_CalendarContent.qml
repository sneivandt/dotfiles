import QtQuick
import QtTest
import "../../../../symlinks/config/quickshell/menus" as Shell

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

        function test_sunday_first_grid_data() {
            return [
                {
                    tag: "month-starts-on-sunday",
                    currentDate: new Date(2026, 1, 1, 14, 32),
                    firstCell: new Date(2026, 1, 1, 12)
                },
                {
                    tag: "month-starts-on-saturday",
                    currentDate: new Date(2026, 7, 19, 14, 32),
                    firstCell: new Date(2026, 6, 26, 12)
                },
                {
                    tag: "previous-year-spillover",
                    currentDate: new Date(2026, 0, 1, 14, 32),
                    firstCell: new Date(2025, 11, 28, 12)
                },
                {
                    tag: "leap-day",
                    currentDate: new Date(2024, 1, 29, 14, 32),
                    firstCell: new Date(2024, 0, 28, 12)
                }
            ];
        }

        function test_sunday_first_grid(data) {
            const calendar = createTemporaryObject(calendarComponent, parent, {
                currentDate: data.currentDate
            });
            verify(calendar !== null);

            const headers = findChild(calendar, "weekdayHeaders");
            verify(headers !== null);
            const weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            for (let column = 0; column < 7; column++)
                compare(headers.itemAt(column).text, weekdays[column]);

            let todayCount = 0;
            for (let index = 0; index < 42; index++) {
                const day = findChild(calendar, "day" + index);
                verify(day !== null);
                const expected = new Date(data.firstCell);
                expected.setDate(expected.getDate() + index);
                compare(day.cellDate.getTime(), expected.getTime());
                compare(day.cellDate.getDay(), index % 7);
                compare(day.inMonth, expected.getMonth() === data.currentDate.getMonth());
                compare(day.today, expected.getFullYear() === data.currentDate.getFullYear() && expected.getMonth() === data.currentDate.getMonth() && expected.getDate() === data.currentDate.getDate());
                if (day.today)
                    todayCount++;
            }
            compare(todayCount, 1);
        }
    }
}
