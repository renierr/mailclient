pragma Singleton

import QtQuick

// In-place ListModel updates.
//
// Why this exists: every feed reload used to do `model.clear()` followed by an
// `append()` per row. That destroys and recreates every delegate, and it also
// invalidates every object handed out by `ListModel.get()` -- those are
// QObjects owned by the model, so a pane holding one (the reader pane held the
// selected message) was left dereferencing freed memory. Clicking the same row
// repeatedly segfaulted the app that way.
//
// `sync()` instead aligns the model with the wanted rows: it removes what is
// gone, inserts what is new, moves what shifted, and writes only the roles
// whose value actually changed. A reload where nothing changed touches no
// delegate at all.
//
// `rows` must be plain JavaScript objects (JSON.parse output, or projections of
// it) -- never `otherModel.get(i)`, for the lifetime reason above.
QtObject {
    id: util

    function indexOfKey(model, key, value) {
        for (var i = 0; i < model.count; i++) {
            if (model.get(i)[key] === value)
                return i
        }
        return -1
    }

    // Write back only the roles that differ, so an unchanged row emits no
    // dataChanged and the delegate does no work.
    function updateRow(model, index, row) {
        var cur = model.get(index)
        var changed = null
        for (var k in row) {
            if (cur[k] !== row[k]) {
                if (changed === null)
                    changed = {}
                changed[k] = row[k]
            }
        }
        if (changed !== null)
            model.set(index, changed)
    }

    // Align `model` with `rows`, identifying rows by the `key` role.
    function sync(model, rows, key) {
        var i
        if (!rows)
            rows = []

        // Drop rows that are gone. Back to front, so indices stay valid.
        var wanted = {}
        for (i = 0; i < rows.length; i++)
            wanted[rows[i][key]] = true
        for (i = model.count - 1; i >= 0; i--) {
            if (wanted[model.get(i)[key]] !== true)
                model.remove(i)
        }

        // Walk the wanted order: insert what is missing, move what moved,
        // update what stayed.
        for (i = 0; i < rows.length; i++) {
            var at = util.indexOfKey(model, key, rows[i][key])
            if (at === -1) {
                model.insert(i, rows[i])
            } else {
                if (at !== i)
                    model.move(at, i, 1)
                util.updateRow(model, i, rows[i])
            }
        }
    }
}
