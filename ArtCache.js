.pragma library

// Artwork kept on disk by the engine, so pictures come back at once after
// a restart instead of from the network. Shared by every ArtMark: each URL
// is asked about once, and anyone waiting on it hears the answer.

var engine = null
// URL → what to show: the local file, or the URL itself when the engine
// couldn't keep it.
var known = {}
var waiting = {}
// Asked before the engine was up: shown from the web for now, and moved
// to the disk copy once it is.
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

// Calls `done` with what to show for `url`: the cached file once there is
// one; the web address when the engine isn't running or can't fetch it.
function resolve(url, done) {
    if (!url || url.indexOf("http") !== 0) {
        done(url || "")
        return
    }
    if (known[url] !== undefined) {
        done(known[url])
        return
    }
    if (!engine || !engine.online) {
        done(url)
        if (engine)
            later.push({ url: url, done: done })
        return
    }
    if (waiting[url]) {
        waiting[url].push(done)
        return
    }
    waiting[url] = [done]
    engine.request("art", { url: url }, function (ok, result) {
        var shown = ok && result && result.path ? "file://" + result.path : url
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
