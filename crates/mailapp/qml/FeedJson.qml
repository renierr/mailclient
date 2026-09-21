pragma Singleton

import QtQuick

// Parsing the JSON payloads that cross into QML, in one place.
//
// Every feed getter in Rust returns valid JSON or its empty shape ("[]" /
// "{}") -- a failure inside `mailcore` becomes that shape, never a parse
// error and never an empty string. This wrapper makes the contract visible
// instead of assumed, and gives the one case it does not cover -- a payload
// that will not parse, i.e. a bug on the producing side -- a single place to
// surface. It replaces a mix of scattered try/catch blocks and bare
// `JSON.parse` calls, the latter of which threw mid-reload and abandoned
// whatever the rest of the function was going to do.
//
// For bridge feeds and for `runJavaScript` results out of WebEngine. NOT for
// job status strings (`job_finished`), where a non-JSON value is an ordinary
// error message and telling the two apart is the caller's actual logic --
// those keep their own try/catch.
//
// `fallback` is what the caller can carry on with: `[]` for a list feed,
// `({})` for an object one.
//
// Never log the payload: feeds carry message bodies, subjects and addresses
// (see AGENT.md, Security & Privacy). The error and the size are enough to
// find the bug.
QtObject {
    function parse(text, fallback) {
        if (text === undefined || text === null || text === "")
            return fallback
        try {
            var parsed = JSON.parse(text)
            return (parsed === null || parsed === undefined) ? fallback : parsed
        } catch (e) {
            console.warn("bridge payload did not parse:", e,
                         "- length", String(text).length)
            return fallback
        }
    }
}
