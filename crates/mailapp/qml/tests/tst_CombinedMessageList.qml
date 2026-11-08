import QtQuick
import QtTest
import ".."

TestCase {
    name: "CombinedMessageList"
    when: windowShown
    width: 400
    height: 600

    MessageList {
        id: list
        width: 400
        height: 600
        combined: true
        folderName: "Combined Inbox"
    }

    SignalSpy {
        id: actions
        target: list
        signalName: "combinedAction"
    }

    function test_duplicate_uids_keep_their_folder_for_swipes() {
        var a = {account_id: 1, folder_id: 10, uid: 7, folder: "INBOX", subject: "A", from: "a@example.com", account_email: "a@example.com", date: "10:00", snippet: "First", unread: true, starred: false, delete_is_permanent: false};
        var b = {account_id: 2, folder_id: 20, uid: 7, folder: "INBOX", subject: "B", from: "b@example.org", account_email: "b@example.org", date: "11:00", snippet: "Second", unread: true, starred: false, delete_is_permanent: true};
        list.messages = [a, b];
        list.rebuildFiltered();
        var byKey = list.sourceByKey();
        compare(byKey["10:7"].account_id, 1);
        compare(byKey["20:7"].account_id, 2);
        actions.clear();
        list.requestSwipe(7, "INBOX", "archive", list.displayRow(b));
        tryCompare(actions, "count", 1);
        compare(actions.signalArguments[0][0].folder_id, 20);
        compare(actions.signalArguments[0][0].account_id, 2);
        compare(actions.signalArguments[0][1], "archive");
        actions.clear();
        list.menuCombined = list.combinedTarget(list.displayRow(a));
        list.combinedMenuAction("delete");
        tryCompare(actions, "count", 1);
        compare(actions.signalArguments[0][0].folder_id, 10);
        compare(actions.signalArguments[0][0].delete_is_permanent, false);
        list.combined = false;
        list.messages = [a];
        list.rebuildFiltered();
        compare(list.indexOfKey("7"), 0);
        compare(list.indexOfKey("20:7"), -1);
    }
}
