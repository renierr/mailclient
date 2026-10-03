import QtQuick
import QtQuick.Dialogs
import QtCore

// "Save as .eml" for one message, shared by the list's row menu and the
// reader. Naming, attachment download and writing are mailcore's
// (`suggested_eml_name`, `export_message`); this only asks where.
FileDialog {
    id: root

    property var backend
    property string folderPath: ""
    property int uid: -1

    signal statusMessage(string text)

    title: qsTr("Save as .eml")
    fileMode: FileDialog.SaveFile
    nameFilters: [qsTr("Mail files (*.eml)"), qsTr("All files (*)")]

    // `folderPath` names the message's folder; empty = the open folder.
    function openFor(folderPath, uid) {
        if (!root.backend || uid < 0)
            return;
        root.folderPath = folderPath;
        root.uid = uid;
        var dir = StandardPaths.writableLocation(StandardPaths.DownloadLocation).toString().replace(/\\/g, "/");
        if (dir.indexOf("file:") !== 0)
            dir = "file://" + (dir.length >= 2 && dir[1] === ":" ? "/" : "") + dir;
        root.selectedFile = dir.replace(/\/+$/, "") + "/" + encodeURIComponent(root.backend.suggested_eml_name(folderPath,
                                                                                                               uid));
        root.open();
    }

    onAccepted: {
        root.statusMessage(qsTr("Exporting…"));
        var r = root.backend.export_message(root.folderPath, root.uid, root.selectedFile.toString());
        if (r !== "")
            root.statusMessage(r);
    }
}
