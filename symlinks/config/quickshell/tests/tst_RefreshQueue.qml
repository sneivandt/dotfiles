import QtQuick
import QtTest
import ".." as Shell

Item {
    Component {
        id: processesComponent

        QtObject {
            id: processes

            property bool statusRunning: false
            property bool actionRunning: false
            property int queriesStarted: 0
            property var queue: Shell.RefreshQueue {
                blocked: processes.statusRunning || processes.actionRunning
                onReady: {
                    processes.queriesStarted++;
                    processes.statusRunning = true;
                }
            }
        }
    }

    TestCase {
        name: "RefreshQueue"

        function makeProcesses() {
            return createTemporaryObject(processesComponent, parent);
        }

        function test_idle_refresh_starts_immediately() {
            const processes = makeProcesses();
            processes.queue.request();
            compare(processes.queriesStarted, 1);
            compare(processes.statusRunning, true);
            compare(processes.queue.pending, false);
        }

        function test_action_completion_during_status_query_keeps_refresh() {
            const processes = makeProcesses();
            processes.queue.request();
            processes.actionRunning = true;
            processes.actionRunning = false;
            processes.queue.request();
            compare(processes.queriesStarted, 1);
            compare(processes.queue.pending, true);

            processes.statusRunning = false;
            tryCompare(processes, "queriesStarted", 2);
            compare(processes.queue.pending, false);
        }

        function test_repeated_refreshes_coalesce_while_action_runs() {
            const processes = makeProcesses();
            processes.actionRunning = true;
            processes.queue.request();
            processes.queue.request();
            processes.queue.request();
            compare(processes.queriesStarted, 0);

            processes.actionRunning = false;
            tryCompare(processes, "queriesStarted", 1);
            compare(processes.queue.pending, false);
            processes.statusRunning = false;
            wait(0);
            compare(processes.queriesStarted, 1);
        }

        function test_exit_handler_can_start_action_before_refresh_drains() {
            const processes = makeProcesses();
            processes.statusRunning = true;
            processes.queue.request();
            processes.statusRunning = false;
            processes.actionRunning = true;
            wait(0);
            compare(processes.queriesStarted, 0);
            compare(processes.queue.pending, true);

            processes.actionRunning = false;
            tryCompare(processes, "queriesStarted", 1);
        }
    }
}
