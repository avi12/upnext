package io.github.zytocc.upnext.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.Proxy
import java.net.URI
import java.net.URISyntaxException
import java.net.URL

/** Something the computer refused, or couldn't be reached for, in words
 *  the app shows as they are. */
class UpNextException(message: String, val status: Int = 0) : Exception(message)

/** The answer to asking: the device shows [matchCode] while it waits. */
@Serializable
data class Asked(
    val id: String,
    val secret: String,
    @SerialName("match_code") val matchCode: String,
    @SerialName("expires_at") val expiresAt: Long,
    val computer: String,
    @SerialName("computer_id") val computerId: String,
) {
    /** Kept as plain strings in a fixed order, for Android to keep across a
     *  turn of the screen; [fromState] reads it back. */
    fun toState(): Array<String> = arrayOf(id, secret, matchCode, expiresAt.toString(), computer, computerId)

    companion object {
        /** [toState]'s strings as a request again; null for anything else. */
        fun fromState(a: Array<String>): Asked? =
            if (a.size == 6) a[3].toLongOrNull()?.let { Asked(a[0], a[1], a[2], it, a[4], a[5]) } else null
    }
}

/** Where a request stands. */
enum class Status { WAITING, ALLOWED, DENIED, EXPIRED }

/** The computer's answer, collected with the request's secret. */
data class Answer(val status: Status, val allowed: Allowed? = null)

/** What an Allow, or a code, gives the app: its token, and where to use it. */
data class Allowed(
    val tokenId: Long,
    val deviceName: String,
    val token: String,
    /** `public_url`, when the computer has one: the token works only there. */
    val address: String?,
    val computer: String?,
    /** The computer's install id: every UpNext that hands an app a token sends it. */
    val computerId: String,
)

/**
 * The calls the app makes before it opens UpNext's own pages: is it UpNext,
 * ask to be allowed, collect the answer, pair with a code, check a saved
 * token, and revoke it. Plain `HttpURLConnection`, straight to the computer
 * whatever proxy the phone has, as the desktop window does; every call
 * blocks, so the app makes them off the main thread. Plain HTTP goes only to
 * addresses on the home network ([Addresses.plainHttpAllowed]).
 */
class PairClient(
    private val timeoutMillis: Int = 8000,
    private val userAgent: String = "UpNext-Android",
    /** A host's addresses, for the plain HTTP rule; tests give their own. */
    private val resolve: (String) -> List<InetAddress> = { InetAddress.getAllByName(it).toList() },
) {
    private val json = Json { ignoreUnknownKeys = true }

    /** What answers at [address], with no token: an error when it isn't
     *  UpNext. */
    fun health(address: String): Health = healthAt(address, pin(address))

    private fun healthAt(address: String, via: InetAddress?): Health {
        val (status, body) = call("GET", "$address/api/health", via = via)
        val app = field(body, "app")
        if (status != 200 || app != "upnext") throw UpNextException("Nothing answers as UpNext at $address.", status)
        return Health(field(body, "version") ?: "unknown", field(body, "id"))
    }

    /** Ask to be allowed: the computer shows the request with the match
     *  number. [expectedId] is the install id the computer was found with,
     *  when it was: another UpNext answering at [address] isn't asked. */
    fun ask(address: String, deviceName: String, kind: String, expectedId: String? = null): Asked {
        val via = pin(address)
        if (expectedId != null) expect(address, healthAt(address, via).id, expectedId)
        val body = JsonObject(mapOf("name" to JsonPrimitive(deviceName), "kind" to JsonPrimitive(kind), "wants" to JsonPrimitive("token")))
        val (status, text) = call("POST", "$address/api/v1/pair/request", body = body.toString(), via = via)
        if (status != 201) throw refusal(status, text)
        val asked = decode(Asked.serializer(), text)
        if (expectedId != null) expect(address, asked.computerId, expectedId)
        return asked
    }

    /** The answer so far. A request that is gone (it ran out, or was
     *  collected already) is [Status.EXPIRED]. */
    fun collect(address: String, asked: Asked): Answer {
        val (status, text) = call("GET", "$address/api/v1/pair/request/${asked.id}", headers = mapOf("UpNext-Pair-Secret" to asked.secret))
        if (status == 404) return Answer(Status.EXPIRED)
        if (status != 200) throw refusal(status, text)
        val got = decode(Collected.serializer(), text)
        return when (got.status) {
            "waiting" -> Answer(Status.WAITING)
            "denied" -> Answer(Status.DENIED)
            "allowed" -> {
                val token = got.token ?: throw UpNextException("The computer allowed this device but sent no token: update UpNext there. $LISTED")
                Answer(Status.ALLOWED, Allowed(got.id ?: 0, got.name.orEmpty(), token, usable(got.address), asked.computer, asked.computerId))
            }
            else -> Answer(Status.EXPIRED)
        }
    }

    /** Pair with the six-digit code from Settings, typed or from its QR
     *  code; [expectedId] as for [ask]. */
    fun pairWithCode(address: String, code: String, deviceName: String, expectedId: String? = null): Allowed {
        val via = pin(address)
        if (expectedId != null) expect(address, healthAt(address, via).id, expectedId)
        val body = JsonObject(mapOf("code" to JsonPrimitive(code), "name" to JsonPrimitive(deviceName), "wants" to JsonPrimitive("token")))
        val (status, text) = call("POST", "$address/api/v1/pair", body = body.toString(), via = via)
        if (status != 201) throw refusal(status, text)
        val got = decode(Paired.serializer(), text)
        val token = got.token ?: throw UpNextException("This computer's UpNext is too old to pair the app with a code: update it, or ask to be allowed. $LISTED")
        val computerId = got.computerId ?: throw UpNextException("This computer's UpNext is too old to pair the app with a code: update it, or ask to be allowed. $LISTED")
        if (expectedId != null) expect(address, computerId, expectedId)
        return Allowed(got.id, got.name, token, usable(got.address), got.computer, computerId)
    }

    /**
     * Whether a saved token still works there. The address is asked for its
     * install id first, with no token, so a token never goes to another
     * UpNext that now answers at a saved computer's address (trust on first
     * use, as the desktop window does). Over plain
     * HTTP the name is looked up once, so the token goes to the address that
     * gave the id.
     */
    fun check(address: String, token: String, computerId: String): TokenState {
        val via = try {
            pin(address).also { if (!same(address, computerId, it)) return TokenState.Changed }
        } catch (e: UpNextException) {
            return TokenState.Unreachable(e.message ?: "")
        }
        val (status, text) = try {
            call("GET", "$address/api/v1/tokens", headers = bearer(token), via = via)
        } catch (e: UpNextException) {
            return TokenState.Unreachable(e.message ?: "")
        }
        return when (status) {
            200 -> TokenState.Works
            401 -> TokenState.Revoked
            else -> TokenState.Unreachable(reason(status, text))
        }
    }

    /**
     * Open a saved computer at [saved], the address its token last worked at.
     * When nothing answers there, or another UpNext does, the addresses it was
     * found at since ([SavedComputers.foundAt]) are asked for its install id
     * with no token, and the first that gives it comes back as
     * [Reached.elsewhere]. An id is public, so the token goes there only when
     * the person says to use it ([check] at that address).
     */
    fun reach(saved: String, found: List<String>, token: String, computerId: String): Reached {
        val state = check(saved, token, computerId)
        if (state == TokenState.Works || state == TokenState.Revoked) return Reached(state, saved)
        val elsewhere = found.filter { it != saved }.firstOrNull { address ->
            try {
                health(address).id == computerId
            } catch (e: UpNextException) {
                false
            }
        }
        return Reached(state, saved, elsewhere)
    }

    /** Revoke this device's token there, when the phone forgets the computer.
     *  False when it couldn't (or another UpNext answers there now): the
     *  computer still lists the device then. */
    fun revoke(address: String, token: String, tokenId: Long, computerId: String): Boolean = try {
        val via = pin(address)
        if (!same(address, computerId, via)) {
            false
        } else {
            val (status, _) = call("DELETE", "$address/api/v1/tokens/$tokenId", headers = bearer(token), via = via)
            status == 204 || status == 401 || status == 404
        }
    } catch (e: UpNextException) {
        false
    }

    /** Whether the UpNext at [address] is the computer with [computerId]. One
     *  that gives no id isn't: every UpNext the app pairs with gives it. */
    private fun same(address: String, computerId: String, via: InetAddress?): Boolean = healthAt(address, via).id == computerId

    /** Over plain HTTP, the checked address every call of one step goes to;
     *  null for https://, where the certificate names the computer. */
    private fun pin(address: String): InetAddress? {
        val uri = try {
            URI(address)
        } catch (e: URISyntaxException) {
            throw UpNextException("That isn't an address the phone can open.")
        }
        val host = uri.host ?: throw UpNextException("That isn't an address the phone can open.")
        return if (uri.scheme == "http") homeAddress(host) else null
    }

    private fun expect(address: String, id: String?, expectedId: String) {
        if (id != expectedId) {
            throw UpNextException("Another UpNext answers at $address now, not the computer found there: go back and choose the computer again.")
        }
    }

    /** The computer's `public_url` as the app keeps it, or null when it has
     *  none. Plain HTTP there obeys the same rule as every other call, so the
     *  WebView never opens an address the app wouldn't talk to itself. */
    private fun usable(given: String?): String? {
        if (given == null) return null
        // The computer made the device before it answered, so a refusal here
        // leaves it listed there.
        val origin = Addresses.fromTyped(given)?.takeIf { given.contains("://") }
            ?: throw UpNextException("The computer's public address ($given) can't be used: check it in Settings › Devices and API there. $LISTED")
        if (origin.startsWith("http://")) {
            try {
                homeAddress(URI(origin).host)
            } catch (e: UpNextException) {
                throw UpNextException("${e.message} $LISTED")
            }
        }
        return origin
    }

    /**
     * The address to reach [host] at over plain HTTP: resolved once, and only
     * when every address is on the home network ([Addresses.plainHttpAllowed]).
     * The connection goes to that address, so a second lookup can't answer
     * with another.
     */
    private fun homeAddress(host: String): InetAddress {
        val name = host.removePrefix("[").removeSuffix("]")
        val addresses = try {
            resolve(name)
        } catch (e: IOException) {
            throw UpNextException("Couldn't find $name: is this device on the same network?")
        }
        if (!Addresses.plainHttpAllowed(addresses)) {
            throw UpNextException("$name isn't on the home network, so this device won't use plain HTTP there, where anyone on the way could read what it sends: use its https:// address.")
        }
        return addresses.first()
    }

    /** [target] with [ip] in place of its name, zone and all, so a link-local
     *  address typed with its interface still connects. */
    internal fun direct(target: URL, ip: InetAddress): URL = try {
        URI("http", null, ip.hostAddress, target.port, target.path.ifEmpty { "/" }, target.query, null).toURL()
    } catch (e: URISyntaxException) {
        throw UpNextException("That isn't an address the phone can open.")
    } catch (e: IOException) {
        throw UpNextException("That isn't an address the phone can open.")
    }

    private fun bearer(token: String) = mapOf("Authorization" to "Bearer $token")

    private fun refusal(status: Int, text: String) = UpNextException(reason(status, text), status)

    private fun reason(status: Int, text: String): String =
        field(text, "error")?.let { it.replaceFirstChar(Char::uppercaseChar) + if (it.endsWith(".")) "" else "." }
            ?: "The computer answered $status."

    private fun field(text: String, key: String): String? = try {
        (json.parseToJsonElement(text) as? JsonObject)?.get(key)?.jsonPrimitive?.contentOrNull
    } catch (e: SerializationException) {
        null
    } catch (e: IllegalArgumentException) {
        null
    }

    private fun <T> decode(serializer: kotlinx.serialization.KSerializer<T>, text: String): T = try {
        json.decodeFromString(serializer, text)
    } catch (e: SerializationException) {
        throw UpNextException("The computer's answer couldn't be read: update UpNext there.")
    } catch (e: IllegalArgumentException) {
        throw UpNextException("The computer's answer couldn't be read: update UpNext there.")
    }

    private fun call(method: String, url: String, headers: Map<String, String> = emptyMap(), body: String? = null, via: InetAddress? = null): Pair<Int, String> {
        val target = try {
            URI(url).toURL()
        } catch (e: URISyntaxException) {
            throw UpNextException("That isn't an address the phone can open.")
        } catch (e: IllegalArgumentException) {
            throw UpNextException("That isn't an address the phone can open.")
        }
        // Plain HTTP goes to the address that was checked, with the name the
        // person gave in the Host header, as a browser would send it.
        val (connectTo, hostHeader) = if (target.protocol == "http") direct(target, via ?: homeAddress(target.host)) to target.authority else target to null
        val conn = try {
            connectTo.openConnection(Proxy.NO_PROXY) as HttpURLConnection
        } catch (e: IOException) {
            throw UpNextException("That isn't an address the phone can open.")
        }
        try {
            conn.requestMethod = method
            conn.connectTimeout = timeoutMillis
            conn.readTimeout = timeoutMillis
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("User-Agent", userAgent)
            hostHeader?.let { conn.setRequestProperty("Host", it) }
            headers.forEach { (k, v) -> conn.setRequestProperty(k, v) }
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            }
            val status = conn.responseCode
            val stream = if (status >= 400) conn.errorStream else conn.inputStream
            val text = stream?.use { it.readBytes().toString(Charsets.UTF_8) }.orEmpty()
            return status to text
        } catch (e: IOException) {
            throw UpNextException("Couldn't reach ${target.host}: is UpNext running there, and this device on the same network?")
        } finally {
            conn.disconnect()
        }
    }

    private companion object {
        const val LISTED = "The computer lists this device already: remove it there in Settings › Devices and API."
    }

    @Serializable
    private data class Collected(
        val status: String,
        val id: Long? = null,
        val name: String? = null,
        val token: String? = null,
        val address: String? = null,
    )

    @Serializable
    private data class Paired(
        val id: Long,
        val name: String,
        val token: String? = null,
        val address: String? = null,
        val computer: String? = null,
        @SerialName("computer_id") val computerId: String? = null,
    )
}

/** What answers at an address: UpNext's version and its install id (null
 *  from an older UpNext). */
data class Health(val version: String, val id: String?)

/** Where a saved computer was reached, and what its token did there. */
data class Reached(
    val state: TokenState,
    val address: String,
    /** Where the computer answers with its id instead, for the person to
     *  confirm before its token goes there. */
    val elsewhere: String? = null,
)

/** What checking a saved token found. */
sealed class TokenState {
    object Works : TokenState()

    /** Another UpNext answers at the saved address now; its token wasn't sent. */
    object Changed : TokenState()

    /** Revoked on the computer (or another install's): ask again. */
    object Revoked : TokenState()

    data class Unreachable(val reason: String) : TokenState()
}
