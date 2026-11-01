import QtQuick
import QtTest
import Quickshell
import "../.." as Shell

Item {
    width: 500
    height: 700

    Component { id: stateComponent; Shell.NotificationState {} }
    Component {
        id: contentComponent
        Shell.NotificationContent { width: 180 }
    }
    Component {
        id: menuComponent
        Shell.NotificationMenu { anchorItem: null }
    }

    TestCase {
        name: "Notifications"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Only inert process mocks may be used.");
        }
        function makeState() {
            const state = createTemporaryObject(stateComponent, parent);
            verify(state !== null);
            return state;
        }
        function query(state) {
            return findChild(state, "notificationQueryProcess");
        }
        function snapshot() {
            return JSON.stringify({ok: true, active: [], history: [
                {app: "Fixture", summary: "Title", body: "Body", critical: false}
            ]});
        }
        function test_queries_only_while_open_and_serializes_refreshes() {
            const state = makeState();
            compare(query(state).attempts, 0);
            state.active = true;
            tryCompare(query(state), "running", true);
            compare(query(state).attempts, 1);
            state.refresh();
            state.refresh();
            compare(query(state).attempts, 1);
            query(state).finish(0, snapshot());
            tryCompare(query(state), "attempts", 2);
            query(state).finish(0, snapshot());
            compare(state.available, true);
            compare(state.history.length, 1);
            state.active = false;
            compare(state.history.length, 0);
            state.refresh();
            compare(query(state).attempts, 2);
        }
        function test_failure_and_recovery() {
            const state = makeState();
            state.active = true;
            tryCompare(query(state), "running", true);
            query(state).failStart();
            compare(state.loading, false);
            compare(state.available, false);
            verify(state.error.includes("Cannot start"));
            state.refresh();
            query(state).finish(0, "invalid");
            compare(state.available, false);
            verify(state.error.length > 0);
            state.refresh();
            query(state).finish(1, snapshot());
            compare(state.available, false);
            state.refresh();
            query(state).finish(0, snapshot());
            compare(state.available, true);
            compare(state.error, "");
        }
        function test_late_result_does_not_repopulate_closed_menu() {
            const state = makeState();
            state.active = true;
            tryCompare(query(state), "running", true);
            state.active = false;
            query(state).finish(0, snapshot());
            compare(state.history.length, 0);
            compare(state.notifications.length, 0);
        }
        function test_narrow_content_long_text_empty_and_failure() {
            const state = makeState();
            const content = createTemporaryObject(contentComponent, parent, {state: state});
            verify(content !== null);
            state.available = true;
            wait(50);
            const emptyHeight = content.implicitHeight;
            state.history = [{app: "Long application ".repeat(20),
                summary: "<b>Literal title</b>".repeat(20), body: "Long body ".repeat(100), critical: true}];
            wait(0);
            tryVerify(() => content.implicitHeight > emptyHeight);
            verify(content.childrenRect.width <= content.width + 1);
            state.available = false;
            state.error = "Notifications unavailable";
            wait(0);
            tryVerify(() => content.implicitHeight < emptyHeight + 100);
            const menu = createTemporaryObject(menuComponent, parent, {notifications: state});
            verify(menu !== null);
            verify(menu.isNotificationMenu);
        }
    }
}
