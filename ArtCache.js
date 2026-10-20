.pragma library

// Artwork kept on disk by the engine, so pictures come back at once after
// a restart instead of from the network. Shared by every ArtMark: each URL
// is asked about once, and anyone waiting on it hears the answer.
//
// Pictures only ever come from the engine's copy. Their addresses come
// from feeds and directories, and the engine only fetches public ones;
// loading the address here instead would skip that check. So a picture the
// engine can't (or won't) fetch shows the plain swatch, and one asked for
// before the engine is up waits for it.

var engine = null
// URL → the local file that shows it.
var known = {}
var waiting = {}
// Asked before the engine was up: answered once it is.
var later = []

function setEngine(e) {
    engine = e
    engine.onlineChanged.connect(function () {
        if (!engine.online)
            return
        var asked = later
        later = []
        for (var i = 0; i < asked.length; i++)
            resolve(asked[i].url, asked[i].done)
    })
}

// Calls `done` with the local file for `url`, or "" while there is none.
function resolve(url, done) {
    if (!url) {
        done("")
        return
    }
    // Web addresses only: a local file named by a feed or a directory never
    // loads.
    if (url.indexOf("https://") !== 0 && url.indexOf("http://") !== 0) {
        done("")
        return
    }
    if (known[url] !== undefined) {
        done(known[url])
        return
    }
    done("")
    if (!engine || !engine.online) {
        later.push({ url: url, done: done })
        return
    }
    if (waiting[url]) {
        waiting[url].push(done)
        return
    }
    waiting[url] = [done]
    engine.request("art", { url: url }, function (ok, result) {
        // The engine stopped before answering: ask again once it's back, as
        // if it had been asked before it was up. Pages stay built while
        // hidden, so nothing else would ask for this picture again.
        if (!ok && !engine.online) {
            var stranded = waiting[url] || []
            delete waiting[url]
            for (var j = 0; j < stranded.length; j++)
                later.push({ url: url, done: stranded[j] })
            return
        }
        var shown = ok && result && result.path ? "file://" + result.path : ""
        // A failure isn't kept: the next picture to ask tries again.
        if (shown !== "")
            known[url] = shown
        var calls = waiting[url] || []
        delete waiting[url]
        for (var i = 0; i < calls.length; i++) {
            try {
                calls[i](shown)
            } catch (e) {
                // Its picture went away while this was fetched.
            }
        }
    })
}
