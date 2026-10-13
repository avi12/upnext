package io.github.zytocc.upnext.core

import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.net.URI
import java.net.URISyntaxException

/** UpNext's port when an address names none, as `upnext serve` listens by default. */
const val DEFAULT_PORT = 8787

/** What a scanned QR code holds: the computer's address and, when Settings
 *  showed a pairing code beside it, the code from its `#pair=` fragment. */
data class Scanned(val address: String, val code: String?)

/** Addresses as people type them and as UpNext's QR codes carry them. */
object Addresses {
    /**
     * The address someone typed, as `scheme://host[:port]`, or null when it
     * isn't one. The same rules as the desktop window's (`network::address_of`
     * in the server): `den.lan` is `http://den.lan:8787`, a port or a scheme
     * given is kept, and a path or query means it isn't an UpNext address.
     */
    fun fromTyped(typed: String): String? {
        val trimmed = typed.trim().trimEnd('/')
        if (trimmed.isEmpty() || trimmed.any { it.isWhitespace() }) return null
        val hasScheme = trimmed.contains("://")
        val uri = parse(if (hasScheme) trimmed else "http://$trimmed") ?: return null
        if (!uri.rawPath.isNullOrEmpty() || uri.rawQuery != null) return null
        val port = if (!hasScheme && uri.port == -1) DEFAULT_PORT else uri.port
        return origin(uri, port)
    }

    /**
     * A QR code from Settings › Devices and API: the address, with the code
     * in its fragment when one was waiting (`http://den.lan:8787/#pair=042917`,
     * or `/setup#pair=…` from big screen's first run). Null for anything
     * else, such as another app's QR code.
     */
    fun fromQr(text: String): Scanned? {
        val uri = parse(text.trim()) ?: return null
        if (!uri.rawPath.isNullOrEmpty() && !uri.rawPath.startsWith("/")) return null
        val address = origin(uri, uri.port) ?: return null
        val code = uri.rawFragment
            ?.split('&')
            ?.firstNotNullOfOrNull { it.removePrefix("pair=").takeIf { v -> v != it } }
            ?.takeIf { it.length == 6 && it.all(Char::isDigit) }
        return Scanned(address, code)
    }

    /**
     * Whether the phone may talk to a host with these addresses over plain
     * HTTP: only when every one is private (the home network's ranges,
     * Tailscale's 100.64.0.0/10, link-local, or the phone itself), the rule
     * the server takes a token by over plain HTTP (`listen::is_private`).
     * Anything else is reached with https://, since anyone on the way could
     * read a token sent in the clear.
     */
    fun plainHttpAllowed(addresses: List<InetAddress>): Boolean = addresses.isNotEmpty() && addresses.all(::isPrivate)

    fun isPrivate(ip: InetAddress): Boolean = when (ip) {
        is Inet4Address -> {
            val a = ip.address[0].toInt() and 0xff
            val b = ip.address[1].toInt() and 0xff
            ip.isLoopbackAddress || ip.isSiteLocalAddress || ip.isLinkLocalAddress || (a == 100 && b in 64..127)
        }
        is Inet6Address -> {
            val first = ip.address[0].toInt() and 0xff
            ip.isLoopbackAddress || ip.isLinkLocalAddress || (first and 0xfe) == 0xfc
        }
        else -> false
    }

    /**
     * Whether a typed or scanned address may be on this network, which an
     * app that targets Android 17 needs the Nearby devices permission to
     * reach, unless the system's picker found it. An IP address is judged by
     * its range: the home network's ranges and link-local count, while the
     * phone itself and Tailscale's 100.64.0.0/10 (a VPN, which the rule
     * leaves out) don't. A name may resolve to an address at home, and
     * looking it up here would be a second lookup before the one the
     * connection makes, so a name counts.
     */
    fun mayBeOnThisNetwork(address: String): Boolean {
        val host = parse(address)?.host?.removePrefix("[")?.removeSuffix("]") ?: return false
        val literal = IPV4.matches(host) || (host.contains(':') && host.all { it.isLetterOrDigit() || it == ':' || it == '.' || it == '%' })
        if (!literal) return true
        // A literal is parsed without a lookup.
        val ip = try {
            InetAddress.getByName(host)
        } catch (e: Exception) {
            return true
        }
        return when (ip) {
            is Inet4Address -> ip.isSiteLocalAddress || ip.isLinkLocalAddress
            is Inet6Address -> ip.isLinkLocalAddress || ((ip.address[0].toInt() and 0xff) and 0xfe) == 0xfc
            else -> false
        }
    }

    private val IPV4 = Regex("""\d{1,3}(\.\d{1,3}){3}""")

    private fun parse(text: String): URI? = try {
        URI(text)
    } catch (e: URISyntaxException) {
        null
    }

    private fun origin(uri: URI, port: Int): String? {
        val scheme = uri.scheme?.lowercase() ?: return null
        if (scheme != "http" && scheme != "https") return null
        if (uri.rawUserInfo != null) return null
        val host = uri.host?.lowercase()?.takeIf { it.isNotEmpty() } ?: return null
        val defaultPort = if (scheme == "https") 443 else 80
        val shownPort = if (port == -1 || port == defaultPort) "" else ":$port"
        return "$scheme://$host$shownPort"
    }
}
