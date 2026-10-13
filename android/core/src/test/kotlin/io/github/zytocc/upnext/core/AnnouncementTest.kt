package io.github.zytocc.upnext.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.net.InetAddress

class AnnouncementTest {
    private fun txt(vararg pairs: Pair<String, String>): Map<String, ByteArray?> = pairs.associate { (k, v) -> k to v.toByteArray() }

    private fun ip(text: String): InetAddress = InetAddress.getByName(text)

    @Test
    fun aRecordUpNextMakes() {
        val attributes = txt("v" to "1", "id" to "4f2a9c01d3e85b67", "name" to "den", "tls" to "0", "ver" to "0.1.0")
        val found = Announcement.read(attributes, "upnext-4f2a9c01d3e8", listOf(ip("192.168.0.42")), 8787)
        assertEquals(Found("4f2a9c01d3e85b67", "den", "0.1.0", "http://192.168.0.42:8787"), found)

        val tls = Announcement.read(attributes + txt("tls" to "1"), "x", listOf(ip("fd00::42")), 8443)
        assertEquals("https://[fd00:0:0:0:0:0:0:42]:8443", tls?.address)

        // No name: the service's own.
        val unnamed = Announcement.read(txt("v" to "1", "id" to "abc123"), "upnext-abc123", listOf(ip("192.168.0.43")), 8787)
        assertEquals("upnext-abc123", unnamed?.name)
        assertNull(unnamed?.version)
    }

    @Test
    fun recordsItCantUse() {
        val good = txt("v" to "1", "id" to "abc123")
        val lan = listOf(ip("192.168.0.42"))
        assertNull("another version", Announcement.read(txt("v" to "2", "id" to "abc123"), "x", lan, 8787))
        assertNull("no version", Announcement.read(txt("id" to "abc123"), "x", lan, 8787))
        assertNull("an id with a slash", Announcement.read(txt("v" to "1", "id" to "a/b"), "x", lan, 8787))
        assertNull("no address", Announcement.read(good, "x", emptyList(), 8787))
        assertNull("loopback only", Announcement.read(good, "x", listOf(ip("127.0.0.1")), 8787))
        assertNull("no port", Announcement.read(good, "x", lan, 0))
    }

    @Test
    fun theAddressChosen() {
        val choices = listOf(ip("127.0.0.1"), ip("fe80::1"), ip("2001:db8::5"), ip("100.64.0.42"), ip("192.168.0.42"))
        assertEquals(ip("192.168.0.42"), Announcement.best(choices))
        assertEquals(ip("100.64.0.42"), Announcement.best(choices - ip("192.168.0.42")))
        assertEquals(ip("2001:db8::5"), Announcement.best(listOf(ip("fe80::1"), ip("2001:db8::5"))))
        assertNull(Announcement.best(listOf(ip("fe80::1"), ip("::1"))))
    }

    @Test
    fun theListSavedFirst() {
        val den = Found("den1", "den", "0.1.0", "http://192.168.0.42:8787")
        val attic = Found("attic1", "attic", "0.1.0", "http://192.168.0.43:8787")
        val saved = listOf(Saved("old1", "office", "http://192.168.0.44:8787", lastOpened = 5), Saved("den1", "den (saved)", "http://192.168.0.41:8787", lastOpened = 9))
        val listed = listComputers(saved, listOf(attic, den, den))
        assertEquals(listOf("den1", "old1", "attic1"), listed.map { it.id })
        // A saved computer shows where its token goes, not what an
        // announcement with its id says.
        assertEquals("den (saved)", listed[0].name)
        assertEquals("http://192.168.0.41:8787", listed[0].address)
        assertEquals(den, listed[0].found)
        assertEquals("http://192.168.0.44:8787", listed[1].address)
        assertEquals("attic", listed[2].name)
        assertNull(listed[2].saved)
    }
}
