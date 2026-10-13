package io.github.zytocc.upnext.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** A request waiting for Allow, as the app keeps it across a turn of the screen. */
class AskedStateTest {
    private val asked = Asked("r1", "s3cret", "4821", 1_700_000_300, "den", "den1")

    @Test
    fun keptAndReadBack() {
        assertEquals(asked, Asked.fromState(asked.toState()))
    }

    @Test
    fun anythingElseIsNoRequest() {
        assertNull(Asked.fromState(arrayOf()))
        assertNull(Asked.fromState(asked.toState().copyOf(5).requireNoNulls()))
        assertNull(Asked.fromState(asked.toState() + "extra"))
        assertNull(Asked.fromState(asked.toState().also { it[3] = "soon" }))
    }
}
