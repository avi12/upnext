package io.github.zytocc.upnext.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SavedComputersTest {
    private val den = Saved("den1", "den", "http://192.168.0.42:8787", tokenId = 9, lastOpened = 100)

    @Test
    fun keptAndReadBack() {
        val list = listOf(den, Saved("attic1", "attic", "https://attic.example.org", fixedAddress = true))
        val text = SavedComputers.encode(list)
        assertTrue(text, !text.contains("upnext_"))
        assertEquals(list, SavedComputers.decode(text))
        // A later version's extra fields don't lose the list.
        assertEquals(listOf(den), SavedComputers.decode(text.replace("\"tokenId\"", "\"colour\":\"teal\",\"tokenId\"").let { SavedComputers.encode(SavedComputers.decode(it).take(1)) }))
    }

    @Test
    fun damagedOrMissing() {
        assertEquals(emptyList<Saved>(), SavedComputers.decode(null))
        assertEquals(emptyList<Saved>(), SavedComputers.decode(""))
        assertEquals(emptyList<Saved>(), SavedComputers.decode("{not json"))
        assertEquals(emptyList<Saved>(), SavedComputers.decode("[{\"name\":\"no id\"}]"))
    }

    @Test
    fun putForgetAndTheLastOne() {
        val attic = Saved("attic1", "attic", "http://192.168.0.43:8787", lastOpened = 50)
        var list = SavedComputers.put(listOf(den), attic)
        assertEquals(den, SavedComputers.last(list))
        list = SavedComputers.put(list, attic.copy(lastOpened = 200))
        assertEquals(2, list.size)
        assertEquals("attic1", SavedComputers.last(list)?.id)
        list = SavedComputers.forget(list, "attic1")
        assertEquals(listOf(den), list)
        assertNull(SavedComputers.last(listOf(den.copy(lastOpened = 0))))
    }

    @Test
    fun foundAgainAtANewAddress() {
        val fixed = Saved("tail1", "den", "https://den.tailnet.ts.net", fixedAddress = true)
        val found = listOf(
            Found("den1", "den downstairs", "0.1.0", "http://192.168.0.44:8787"),
            Found("den1", "den downstairs", "0.1.0", "http://192.168.0.44:8787"),
            Found("tail1", "den", "0.1.0", "http://192.168.0.45:8787"),
        )
        assertEquals(listOf("http://192.168.0.44:8787"), SavedComputers.foundAt(den, found))
        assertEquals("found where it was saved", emptyList<String>(), SavedComputers.foundAt(den, listOf(Found("den1", "den", "0.1.0", den.address))))
        assertEquals(emptyList<String>(), SavedComputers.foundAt(den, emptyList()))
        assertEquals("public_url only", emptyList<String>(), SavedComputers.foundAt(fixed, found))

        // Kept once the person chose it and the token worked there.
        val moved = SavedComputers.moved(listOf(den, fixed), "den1", "http://192.168.0.44:8787")
        assertEquals(den.copy(address = "http://192.168.0.44:8787"), moved[0])
        assertEquals(fixed, moved[1])
        assertEquals("public_url stays", fixed, SavedComputers.moved(listOf(fixed), "tail1", "http://192.168.0.45:8787")[0])
    }
}
