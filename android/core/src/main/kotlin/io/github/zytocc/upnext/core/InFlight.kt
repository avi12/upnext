package io.github.zytocc.upnext.core

/**
 * The one pairing request in flight, asking to be allowed or pairing with a
 * typed or scanned code, kept apart from the screen that started it.
 * Android makes the screen anew when it turns; the new one takes the answer
 * the old one was waiting for rather than asking again, which would show the
 * computer a second number or send a single-use code twice and fail as
 * used. A newer request replaces an older one, whose answer is then dropped.
 *
 * [run] does the call off the main thread and [post] brings its answer back
 * to it; everything else is called on the main thread.
 */
class InFlight(private val run: (Runnable) -> Unit, private val post: (Runnable) -> Unit) {
    enum class Kind { ASK, CODE }

    /** What's in flight, or answered and not yet taken; null otherwise. */
    var kind: Kind? = null
        private set

    private var current: Any? = null
    private var answer: Result<Any>? = null
    private var taker: ((Kind, Result<Any>) -> Unit)? = null

    fun start(kind: Kind, call: () -> Any) {
        val request = Any()
        current = request
        this.kind = kind
        answer = null
        run(Runnable {
            val result = runCatching(call)
            post(Runnable {
                if (current === request) {
                    answer = result
                    hand()
                }
            })
        })
    }

    /** Answers go to [to] from now on, one that came while no screen was
     *  there included. */
    fun attach(to: (Kind, Result<Any>) -> Unit) {
        taker = to
        hand()
    }

    /** The screen is gone: an answer waits for the next one. */
    fun detach() {
        taker = null
    }

    private fun hand() {
        val result = answer ?: return
        val to = taker ?: return
        val kind = kind ?: return
        current = null
        this.kind = null
        answer = null
        to(kind, result)
    }
}
