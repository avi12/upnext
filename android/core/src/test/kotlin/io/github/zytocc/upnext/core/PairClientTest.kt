package io.github.zytocc.upnext.core

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import java.net.InetAddress
import java.net.InetSocketAddress
import java.util.Collections

/** The pairing calls against a stand-in for UpNext on this machine, never a
 *  real computer. */
class PairClientTest {
    private lateinit var server: HttpServer
    private lateinit var address: String
    private val seen: MutableList<String> = Collections.synchronizedList(mutableListOf())
    private var answers: Map<String, Pair<Int, String>> = emptyMap()
    private val hosts: MutableList<String> = Collections.synchronizedList(mutableListOf())

    @Before
    fun start() {
        server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/") { ex -> answer(ex) }
        server.start()
        address = "http://127.0.0.1:${server.address.port}"
    }

    @After
    fun stop() = server.stop(0)

    private fun answer(ex: HttpExchange) {
        val body = ex.requestBody.readBytes().toString(Charsets.UTF_8)
        hosts.add(ex.requestHeaders.getFirst("Host").orEmpty())
        val headers = listOf("Authorization", "UpNext-Pair-Secret", "Content-Type")
            .mapNotNull { h -> ex.requestHeaders.getFirst(h)?.let { "$h=$it" } }
        seen.add("${ex.requestMethod} ${ex.requestURI} ${headers.joinToString(",")} $body".trim())
        val (status, text) = answers["${ex.requestMethod} ${ex.requestURI.path}"] ?: (404 to """{"error":"not found"}""")
        val bytes = text.toByteArray()
        ex.sendResponseHeaders(status, if (bytes.isEmpty()) -1 else bytes.size.toLong())
        if (bytes.isNotEmpty()) ex.responseBody.use { it.write(bytes) }
        ex.close()
    }

    private val client = PairClient(timeoutMillis = 2000)

    @Test
    fun healthSaysUpNext() {
        answers = mapOf("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","container":false,"id":"4f2a"}"""))
        assertEquals(Health("0.1.0", "4f2a"), client.health(address))
        answers = mapOf("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","container":false}"""))
        assertEquals("before the id", Health("0.1.0", null), client.health(address))
        answers = mapOf("GET /api/health" to (200 to """{"app":"something else"}"""))
        expectRefusal("Nothing answers as UpNext") { client.health(address) }
    }

    @Test
    fun askWaitAndCollectTheToken() {
        val asked = """{"id":"9c1e","secret":"s3cret","match_code":"4821","expires_at":1790000120,"computer":"den","computer_id":"4f2a9c01d3e85b67"}"""
        answers = mapOf(
            "POST /api/v1/pair/request" to (201 to asked),
            "GET /api/v1/pair/request/9c1e" to (200 to """{"status":"waiting"}"""),
        )
        val request = client.ask(address, "Pixel 9", "phone")
        assertEquals("4821", request.matchCode)
        assertEquals("4f2a9c01d3e85b67", request.computerId)
        assertTrue(seen[0], seen[0].contains("\"wants\":\"token\"") && seen[0].contains("\"kind\":\"phone\"") && seen[0].contains("Pixel 9"))
        assertEquals(Answer(Status.WAITING), client.collect(address, request))
        assertTrue(seen[1], seen[1].contains("UpNext-Pair-Secret=s3cret"))

        answers = answers + ("GET /api/v1/pair/request/9c1e" to (200 to """{"status":"allowed","id":9,"name":"Pixel 9","token":"upnext_abc","address":null}"""))
        val allowed = client.collect(address, request)
        assertEquals(Status.ALLOWED, allowed.status)
        assertEquals(Allowed(9, "Pixel 9", "upnext_abc", null, "den", "4f2a9c01d3e85b67"), allowed.allowed)

        answers = answers + ("GET /api/v1/pair/request/9c1e" to (200 to """{"status":"denied","id":null,"name":null,"token":null,"address":null}"""))
        assertEquals(Answer(Status.DENIED), client.collect(address, request))
        answers = answers - "GET /api/v1/pair/request/9c1e"
        assertEquals("gone, or collected already", Answer(Status.EXPIRED), client.collect(address, request))
    }

    @Test
    fun theComputersReasonsAreShown() {
        answers = mapOf("POST /api/v1/pair/request" to (429 to """{"error":"3 devices are already waiting to be allowed: answer them on the computer UpNext runs on first"}"""))
        val e = expectRefusal("3 devices are already waiting") { client.ask(address, "Pixel 9", "phone") }
        assertEquals(429, e.status)
        answers = mapOf("POST /api/v1/pair/request" to (500 to "oops"))
        expectRefusal("The computer answered 500.") { client.ask(address, "Pixel 9", "phone") }
    }

    @Test
    fun pairWithACode() {
        answers = mapOf("POST /api/v1/pair" to (201 to """{"id":7,"name":"Pixel 9","token":"upnext_xyz","address":"https://den.tailnet.ts.net","computer":"den","computer_id":"4f2a"}"""))
        val got = client.pairWithCode(address, "042917", "Pixel 9")
        assertEquals(Allowed(7, "Pixel 9", "upnext_xyz", "https://den.tailnet.ts.net", "den", "4f2a"), got)
        assertTrue(seen[0], seen[0].contains("\"code\":\"042917\"") && seen[0].contains("\"wants\":\"token\""))

        // An UpNext from before the app answers with the cookie alone.
        answers = mapOf("POST /api/v1/pair" to (201 to """{"id":7,"name":"Pixel 9"}"""))
        expectRefusal("too old") { client.pairWithCode(address, "042917", "Pixel 9") }
        answers = mapOf("POST /api/v1/pair" to (401 to """{"error":"that's not the code: 4 tries left"}"""))
        expectRefusal("That's not the code: 4 tries left.") { client.pairWithCode(address, "000000", "Pixel 9") }
    }

    @Test
    fun checkAndRevokeASavedToken() {
        val health = "GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den1"}""")
        answers = mapOf(health, "GET /api/v1/tokens" to (200 to """{"tokens":[],"current":9}"""))
        assertEquals(TokenState.Works, client.check(address, "upnext_abc", "den1"))
        assertTrue(seen[0], seen[0].startsWith("GET /api/health") && !seen[0].contains("Authorization"))
        assertTrue(seen.last(), seen.last().contains("Authorization=Bearer upnext_abc"))
        answers = mapOf(health, "GET /api/v1/tokens" to (401 to """{"error":"that token isn't valid"}"""))
        assertEquals(TokenState.Revoked, client.check(address, "upnext_abc", "den1"))

        answers = mapOf(health, "DELETE /api/v1/tokens/9" to (204 to ""))
        assertTrue(client.revoke(address, "upnext_abc", 9, "den1"))
        assertTrue(seen.last(), seen.last().startsWith("DELETE /api/v1/tokens/9 Authorization=Bearer upnext_abc"))
        answers = mapOf(health, "DELETE /api/v1/tokens/9" to (403 to """{"error":"no"}"""))
        assertFalse(client.revoke(address, "upnext_abc", 9, "den1"))
    }

    @Test
    fun anotherUpNextAtTheAddressGetsNoToken() {
        answers = mapOf(
            "GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"someone-else"}"""),
            "GET /api/v1/tokens" to (200 to """{"tokens":[],"current":9}"""),
            "DELETE /api/v1/tokens/9" to (204 to ""),
        )
        assertEquals(TokenState.Changed, client.check(address, "upnext_abc", "den1"))
        assertFalse(client.revoke(address, "upnext_abc", 9, "den1"))
        assertTrue(seen.toString(), seen.none { it.contains("Authorization") })
    }

    @Test
    fun aComputerFoundAtAnotherAddressGetsNoTokenUntilThePersonSaysSo() {
        answers = mapOf(
            "GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den1"}"""),
            "GET /api/v1/tokens" to (200 to """{"tokens":[],"current":9}"""),
        )
        // A second UpNext, found on the network with the same id.
        var otherId = "den1"
        val other = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        val otherSeen: MutableList<String> = Collections.synchronizedList(mutableListOf())
        other.createContext("/") { ex ->
            otherSeen.add("${ex.requestMethod} ${ex.requestURI.path} ${ex.requestHeaders.getFirst("Authorization").orEmpty()}".trim())
            val (status, text) = when (ex.requestURI.path) {
                "/api/health" -> 200 to """{"app":"upnext","version":"0.1.0","id":"$otherId"}"""
                else -> 200 to """{"tokens":[],"current":9}"""
            }
            ex.sendResponseHeaders(status, text.length.toLong())
            ex.responseBody.use { it.write(text.toByteArray()) }
            ex.close()
        }
        other.start()
        val found = "http://127.0.0.1:${other.address.port}"
        try {
            // The saved address works: the found one isn't asked at all.
            assertEquals(Reached(TokenState.Works, address), client.reach(address, listOf(found), "upnext_abc", "den1"))
            assertTrue(otherSeen.toString(), otherSeen.isEmpty())

            // Another UpNext at the saved address: the found one is asked
            // for its id, with no token, and offered.
            answers = answers + ("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"someone-else"}"""))
            assertEquals(Reached(TokenState.Changed, address, found), client.reach(address, listOf(found), "upnext_abc", "den1"))
            assertEquals(listOf("GET /api/health"), otherSeen.toList())

            // The saved address is gone: offered again, still with no token.
            server.stop(0)
            val gone = client.reach(address, listOf(found), "upnext_abc", "den1")
            assertTrue("$gone", gone.state is TokenState.Unreachable && gone.address == address && gone.elsewhere == found)
            assertTrue(otherSeen.toString(), otherSeen.none { it.contains("Bearer") })
            // Only another id there: nothing to offer.
            otherId = "someone-else"
            assertEquals(null, client.reach(address, listOf(found), "upnext_abc", "den1").elsewhere)

            // The person said to use it: the token goes there now.
            otherId = "den1"
            assertEquals(TokenState.Works, client.check(found, "upnext_abc", "den1"))
            assertTrue(otherSeen.toString(), otherSeen.last() == "GET /api/v1/tokens Bearer upnext_abc")
        } finally {
            other.stop(0)
        }
    }

    @Test
    fun anUpNextThatGivesNoIdIsntTheSavedComputer() {
        answers = mapOf(
            "GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0"}"""),
            "GET /api/v1/tokens" to (200 to """{"tokens":[],"current":9}"""),
            "POST /api/v1/pair/request" to (201 to """{"id":"9c1e","secret":"s3cret","match_code":"4821","expires_at":1790000120,"computer":"den","computer_id":"den1"}"""),
        )
        assertEquals(TokenState.Changed, client.check(address, "upnext_abc", "den1"))
        assertFalse(client.revoke(address, "upnext_abc", 9, "den1"))
        expectRefusal("Another UpNext answers") { client.ask(address, "Pixel 9", "phone", expectedId = "den1") }
        assertTrue(seen.toString(), seen.none { it.contains("Authorization") || it.startsWith("POST") })

        // A code's answer without the id is from an UpNext too old for the app.
        answers = mapOf("POST /api/v1/pair" to (201 to """{"id":7,"name":"Pixel 9","token":"upnext_xyz"}"""))
        expectRefusal("remove it there in Settings") { client.pairWithCode(address, "042917", "Pixel 9") }
    }

    @Test
    fun theComputersPublicAddressObeysThePlainHttpRule() {
        val public = PairClient(timeoutMillis = 2000, resolve = { host ->
            if (host == "den.example.org") listOf(InetAddress.getByName("203.0.113.42")) else InetAddress.getAllByName(host).toList()
        })
        val paired = { url: String -> """{"id":7,"name":"Pixel 9","token":"upnext_xyz","address":"$url","computer":"den","computer_id":"den1"}""" }
        answers = mapOf("POST /api/v1/pair" to (201 to paired("http://den.example.org:8787")))
        val refused = expectRefusal("use its https:// address") { public.pairWithCode(address, "042917", "Pixel 9") }
        // The computer made the device before it answered: say where to remove it.
        assertTrue(refused.message, refused.message!!.contains("remove it there in Settings › Devices and API"))
        answers = mapOf("POST /api/v1/pair" to (201 to paired("https://den.example.org/")))
        assertEquals("https://den.example.org", public.pairWithCode(address, "042917", "Pixel 9").address)
        answers = mapOf("POST /api/v1/pair" to (201 to paired("den.example.org/games")))
        expectRefusal("can't be used") { public.pairWithCode(address, "042917", "Pixel 9") }

        // The same rule when an Allow hands the address over.
        val asked = Asked("9c1e", "s3cret", "4821", 1790000120, "den", "den1")
        answers = mapOf("GET /api/v1/pair/request/9c1e" to (200 to """{"status":"allowed","id":9,"name":"Pixel 9","token":"upnext_abc","address":"http://den.example.org:8787"}"""))
        expectRefusal("use its https:// address") { public.collect(address, asked) }
    }

    @Test
    fun plainHttpConnectsToTheAddressThatWasChecked() {
        val lookups = mutableListOf<String>()
        // A resolver that answers private once, then public.
        val fickle = PairClient(timeoutMillis = 2000, resolve = { host ->
            lookups.add(host)
            listOf(InetAddress.getByName(if (lookups.size == 1) "127.0.0.1" else "203.0.113.42"))
        })
        answers = mapOf("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den1"}"""))
        val port = server.address.port
        assertEquals(Health("0.1.0", "den1"), fickle.health("http://den.lan:$port"))
        assertEquals("one lookup per call", listOf("den.lan"), lookups)
        // The request went to the checked address, named as the person gave it.
        assertTrue(seen.toString(), seen.single().startsWith("GET /api/health"))
        assertTrue(hosts.toString(), hosts.single() == "den.lan:$port")
    }

    @Test
    fun askingAndCodesStopWhenAnotherUpNextAnswers() {
        val asked = """{"id":"9c1e","secret":"s3cret","match_code":"4821","expires_at":1790000120,"computer":"den","computer_id":"den1"}"""
        answers = mapOf(
            "GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"someone-else"}"""),
            "POST /api/v1/pair/request" to (201 to asked),
            "POST /api/v1/pair" to (201 to """{"id":7,"name":"Pixel 9","token":"upnext_xyz","computer":"den","computer_id":"den1"}"""),
        )
        expectRefusal("Another UpNext answers") { client.ask(address, "Pixel 9", "phone", expectedId = "den1") }
        expectRefusal("Another UpNext answers") { client.pairWithCode(address, "042917", "Pixel 9", expectedId = "den1") }
        assertTrue(seen.toString(), seen.none { it.startsWith("POST") })

        // The health check agrees, but the answer names another computer.
        answers = answers + ("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den2"}"""))
        expectRefusal("Another UpNext answers") { client.pairWithCode(address, "042917", "Pixel 9", expectedId = "den2") }
        expectRefusal("Another UpNext answers") { client.ask(address, "Pixel 9", "phone", expectedId = "den2") }
        answers = answers + ("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den1"}"""))
        assertEquals("den1", client.pairWithCode(address, "042917", "Pixel 9", expectedId = "den1").computerId)
        assertEquals("den1", client.ask(address, "Pixel 9", "phone", expectedId = "den1").computerId)
    }

    @Test
    fun plainHttpGoesOnlyToTheHomeNetwork() {
        val public = PairClient(timeoutMillis = 2000, resolve = { host ->
            if (host == "den.example.org") listOf(InetAddress.getByName("203.0.113.42")) else InetAddress.getAllByName(host).toList()
        })
        answers = mapOf("GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den1"}"""))
        val port = server.address.port
        expectRefusal("use its https:// address") { public.health("http://den.example.org:$port") }
        expectRefusal("use its https:// address") { public.pairWithCode("http://den.example.org:$port", "042917", "Pixel 9") }
        val state = public.check("http://den.example.org:$port", "upnext_abc", "den1")
        assertTrue("$state", state is TokenState.Unreachable && state.reason.contains("https://"))
        assertTrue(seen.toString(), seen.isEmpty())
        assertEquals(Health("0.1.0", "den1"), public.health(address))
    }

    @Test
    fun aSavedTokenGoesToTheAddressThatGaveTheId() {
        val lookups = mutableListOf<String>()
        val fickle = PairClient(timeoutMillis = 2000, resolve = { host ->
            lookups.add(host)
            listOf(InetAddress.getByName(if (lookups.size == 1) "127.0.0.1" else "192.168.0.43"))
        })
        answers = mapOf(
            "GET /api/health" to (200 to """{"app":"upnext","version":"0.1.0","id":"den1"}"""),
            "GET /api/v1/tokens" to (200 to """{"tokens":[],"current":9}"""),
            "DELETE /api/v1/tokens/9" to (204 to ""),
        )
        val port = server.address.port
        assertEquals(TokenState.Works, fickle.check("http://den.lan:$port", "upnext_abc", "den1"))
        assertEquals("one lookup for the id and the token", listOf("den.lan"), lookups)
        lookups.clear()
        assertTrue(fickle.revoke("http://den.lan:$port", "upnext_abc", 9, "den1"))
        assertEquals(listOf("den.lan"), lookups)
        assertEquals(4, seen.size)
    }

    @Test
    fun httpsIsReachedByName() {
        val lookups = mutableListOf<String>()
        val counting = PairClient(timeoutMillis = 500, resolve = { host -> lookups.add(host); InetAddress.getAllByName(host).toList() })
        // Nothing listens for TLS there; what matters is that the app didn't
        // look the name up itself, so the certificate check sees the name.
        expectRefusal("Couldn't reach") { counting.health("https://127.0.0.1:${server.address.port}") }
        assertEquals(emptyList<String>(), lookups)
    }

    @Test
    fun aLinkLocalAddressKeepsItsZone() {
        val scoped = java.net.Inet6Address.getByAddress(null, InetAddress.getByName("fe80::42").address, 2)
        val url = client.direct(java.net.URI("http://den.local:8787/api/health").toURL(), scoped)
        assertEquals("[fe80:0:0:0:0:0:0:42%2]", url.host)
        assertEquals(8787, url.port)
        assertEquals("/api/health", url.path)
    }

    @Test
    fun nothingThere() {
        server.stop(0)
        val state = client.check(address, "upnext_abc", "den1")
        assertTrue("$state", state is TokenState.Unreachable && state.reason.startsWith("Couldn't reach 127.0.0.1"))
        assertFalse(client.revoke(address, "upnext_abc", 9, "den1"))
        expectRefusal("Couldn't reach") { client.health(address) }
    }

    private fun expectRefusal(part: String, block: () -> Unit): UpNextException {
        try {
            block()
        } catch (e: UpNextException) {
            assertTrue(e.message, e.message!!.contains(part))
            return e
        }
        fail("expected a refusal with \"$part\"")
        throw AssertionError()
    }
}
