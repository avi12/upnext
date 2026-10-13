package io.github.zytocc.upnext.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The request in flight across a turn of the screen, with the worker and
 *  the main thread played by two queues run by hand. */
class InFlightTest {
    private val worker = ArrayDeque<Runnable>()
    private val main = ArrayDeque<Runnable>()
    private val inFlight = InFlight(worker::addLast, main::addLast)
    private val taken = mutableListOf<Pair<InFlight.Kind, Result<Any>>>()
    private val take: (InFlight.Kind, Result<Any>) -> Unit = { kind, result -> taken.add(kind to result) }

    private fun runAll() {
        while (worker.isNotEmpty() || main.isNotEmpty()) {
            worker.removeFirstOrNull()?.run()
            main.removeFirstOrNull()?.run()
        }
    }

    @Test
    fun anAnswerWaitsForTheNextScreen() {
        var calls = 0
        inFlight.attach(take)
        inFlight.start(InFlight.Kind.CODE) { calls++; "allowed" }
        // The screen turns before the answer comes.
        inFlight.detach()
        runAll()
        assertEquals(InFlight.Kind.CODE, inFlight.kind)
        assertEquals(emptyList<Pair<InFlight.Kind, Result<Any>>>(), taken)
        // The new screen sees it's still in flight, then takes the answer.
        inFlight.attach(take)
        assertEquals(listOf(InFlight.Kind.CODE to Result.success<Any>("allowed")), taken)
        assertEquals(1, calls)
        assertNull(inFlight.kind)
        // Taken once: another screen gets nothing.
        inFlight.attach(take)
        assertEquals(1, taken.size)
    }

    @Test
    fun aNewerRequestReplacesAnOlderOne() {
        inFlight.attach(take)
        inFlight.start(InFlight.Kind.ASK) { "first number" }
        inFlight.start(InFlight.Kind.CODE) { "allowed" }
        runAll()
        assertEquals(listOf(InFlight.Kind.CODE to Result.success<Any>("allowed")), taken)
    }

    @Test
    fun aFailureIsAnAnswerToo() {
        val refused = UpNextException("That code was already used")
        inFlight.attach(take)
        inFlight.start(InFlight.Kind.CODE) { throw refused }
        runAll()
        assertEquals(listOf(InFlight.Kind.CODE to Result.failure<Any>(refused)), taken)
    }
}
