package io.github.zytocc.upnext.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.InetAddress

class AddressesTest {
    @Test
    fun addressesPeopleType() {
        // The same cases as the server's `network::addresses_people_type`.
        assertEquals("http://den.lan:8787", Addresses.fromTyped("den.lan"))
        assertEquals("http://192.168.0.42:9000", Addresses.fromTyped(" 192.168.0.42:9000/ "))
        assertEquals("https://den.tailnet.ts.net", Addresses.fromTyped("https://den.tailnet.ts.net"))
        assertEquals("http://192.168.0.42:8787", Addresses.fromTyped("http://192.168.0.42:8787/"))
        assertEquals("http://[fd00::42]:8787", Addresses.fromTyped("[fd00::42]"))
        assertEquals("http://den.lan:8787", Addresses.fromTyped("DEN.lan"))
        assertEquals("https://den.example.org:8443", Addresses.fromTyped("https://den.example.org:8443"))
        for (bad in listOf("", "den lan", "ftp://den.lan", "http://den.lan/games", "http://den.lan/?x=1", "http://you@den.lan")) {
            assertNull(bad, Addresses.fromTyped(bad))
        }
    }

    @Test
    fun addressesThatMayBeOnThisNetwork() {
        for (home in listOf("http://192.168.0.42:8787", "http://10.0.0.42:8787", "http://172.16.0.42", "http://169.254.0.42:8787", "http://[fd00::42]:8787", "http://[fe80::42]:8787")) {
            assertTrue(home, Addresses.mayBeOnThisNetwork(home))
        }
        // A name may resolve to an address at home; it isn't looked up here.
        for (name in listOf("http://den.lan:8787", "https://den.tailnet.ts.net", "https://den.example.org:8443")) {
            assertTrue(name, Addresses.mayBeOnThisNetwork(name))
        }
        for (away in listOf("http://100.64.0.42:8787", "http://127.0.0.1:8787", "http://[::1]:8787", "https://93.184.215.14", "http://[2001:db8::42]:8787", "not an address")) {
            assertFalse(away, Addresses.mayBeOnThisNetwork(away))
        }
    }

    @Test
    fun qrCodesFromSettings() {
        assertEquals(Scanned("http://192.168.0.42:8787", "042917"), Addresses.fromQr("http://192.168.0.42:8787/#pair=042917"))
        assertEquals(Scanned("http://den.lan:8787", "042917"), Addresses.fromQr("http://den.lan:8787/setup#pair=042917"))
        assertEquals(Scanned("https://den.tailnet.ts.net", null), Addresses.fromQr("https://den.tailnet.ts.net"))
        assertEquals(Scanned("http://den.lan:8787", null), Addresses.fromQr("http://den.lan:8787/#pair=12345"))
        assertNull(Addresses.fromQr("WIFI:S:home;T:WPA;P:secret;;"))
        assertNull(Addresses.fromQr("mailto:someone@example.org"))
        assertNull(Addresses.fromQr("not a code"))
    }

    @Test
    fun plainHttpOnlyAtHome() {
        val ip = { text: String -> InetAddress.getByName(text) }
        for (home in listOf("192.168.0.42", "10.0.0.42", "172.16.0.42", "100.64.0.42", "169.254.0.42", "127.0.0.1", "fd00::42", "fe80::42", "::1")) {
            assertTrue(home, Addresses.plainHttpAllowed(listOf(ip(home))))
        }
        for (away in listOf("203.0.113.42", "198.51.100.42", "100.128.0.42", "172.32.0.42", "2001:db8::42")) {
            assertFalse(away, Addresses.plainHttpAllowed(listOf(ip(away))))
        }
        assertFalse("one public address is enough to refuse", Addresses.plainHttpAllowed(listOf(ip("192.168.0.42"), ip("203.0.113.42"))))
        assertFalse("nothing found", Addresses.plainHttpAllowed(emptyList()))
    }
}
