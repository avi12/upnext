package io.github.zytocc.upnext.core

import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress

/** A computer announcing UpNext on the network (`_upnext._tcp`). */
data class Found(
    /** Its install id, which stays the same when its address or name changes. */
    val id: String,
    val name: String,
    val version: String?,
    val address: String,
)

/** Reading what UpNext announces over mDNS and DNS-SD
 *  (`src/announce.rs`): TXT values `v`, `id`, `name`, `tls` and `ver`. */
object Announcement {
    /** The service type Android's `NsdManager` looks for. */
    const val SERVICE_TYPE = "_upnext._tcp"

    private val ID = Regex("[A-Za-z0-9]{1,64}")

    /**
     * The computer a resolved service describes, or null when it isn't one
     * UpNext can use: another version of the record, no usable id, or no
     * address worth trying. [serviceName] stands in for a missing name.
     */
    fun read(attributes: Map<String, ByteArray?>, serviceName: String, addresses: List<InetAddress>, port: Int): Found? {
        fun text(key: String): String? = attributes[key]?.toString(Charsets.UTF_8)?.trim()?.takeIf { it.isNotEmpty() }
        if (text("v") != "1") return null
        val id = text("id")?.takeIf { ID.matches(it) } ?: return null
        if (port !in 1..65535) return null
        val host = best(addresses) ?: return null
        val scheme = if (text("tls") == "1") "https" else "http"
        val shown = if (host is Inet6Address) "[${plain(host)}]" else plain(host)
        val name = (text("name") ?: serviceName).take(63)
        return Found(id, name, text("ver"), "$scheme://$shown:$port")
    }

    /**
     * The address to use, as the desktop window chooses (`announce::browse`):
     * a home network's IPv4 address first, then any other IPv4, then IPv6
     * that isn't link-local, never loopback. A link-local IPv6 address would
     * need its interface named, which a URL in a WebView can't carry.
     */
    fun best(addresses: List<InetAddress>): InetAddress? {
        val usable = addresses.filterNot { it.isLoopbackAddress || it.isAnyLocalAddress || it.isMulticastAddress }
        return usable.firstOrNull { it is Inet4Address && it.isSiteLocalAddress }
            ?: usable.firstOrNull { it is Inet4Address && !it.isLinkLocalAddress }
            ?: usable.firstOrNull { it is Inet6Address && !it.isLinkLocalAddress }
    }

    private fun plain(address: InetAddress): String = address.hostAddress.substringBefore('%')
}

/** A found computer's place in the list: allowed before, or new. A saved
 *  computer shows the name and address it was saved with, where its token
 *  goes, whatever an announcement with its id says. */
data class Listed(val found: Found?, val saved: Saved?) {
    val id: String get() = saved?.id ?: found!!.id
    val name: String get() = saved?.name ?: found!!.name
    val address: String get() = saved?.address ?: found!!.address
}

/** The list the app's first screen shows: every saved computer, with what
 *  the network says about it now, then the new ones found. */
fun listComputers(saved: List<Saved>, found: List<Found>): List<Listed> {
    val byId = found.associateBy { it.id }
    val known = saved.sortedByDescending { it.lastOpened }.map { Listed(byId[it.id], it) }
    val new = found.filter { f -> saved.none { it.id == f.id } }.distinctBy { it.id }.sortedBy { it.name.lowercase() }
    return known + new.map { Listed(it, null) }
}
