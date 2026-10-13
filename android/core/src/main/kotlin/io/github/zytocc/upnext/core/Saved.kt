package io.github.zytocc.upnext.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json

/**
 * A computer this phone was allowed on. The token itself isn't here: the
 * app keeps it sealed with a key in Android's keystore, under [id].
 */
@Serializable
data class Saved(
    /** The computer's install id, from its announcement and its answer. */
    val id: String,
    val name: String,
    /** Where it was last reached, or `public_url` when it gave one. */
    val address: String,
    /** The token's id in that computer's device list, for revoking it when
     *  the phone forgets the computer. */
    val tokenId: Long? = null,
    /** When it was last opened (Unix seconds), so the newest comes first. */
    val lastOpened: Long = 0,
    /** Whether `address` is `public_url`, which a found address mustn't
     *  replace: the token is taken only at that name. */
    val fixedAddress: Boolean = false,
)

/** The saved computers, as the app keeps them in its private preferences. */
object SavedComputers {
    private val json = Json { ignoreUnknownKeys = true }
    private val serializer = ListSerializer(Saved.serializer())

    fun encode(saved: List<Saved>): String = json.encodeToString(serializer, saved)

    /** What was saved; nothing when it can't be read, so a damaged file
     *  costs the list, not the app. */
    fun decode(text: String?): List<Saved> {
        if (text.isNullOrBlank()) return emptyList()
        return try {
            json.decodeFromString(serializer, text)
        } catch (e: SerializationException) {
            emptyList()
        } catch (e: IllegalArgumentException) {
            emptyList()
        }
    }

    /** [saved] added, or put in place of the one with its id. */
    fun put(list: List<Saved>, saved: Saved): List<Saved> = list.filterNot { it.id == saved.id } + saved

    fun forget(list: List<Saved>, id: String): List<Saved> = list.filterNot { it.id == id }

    /**
     * Where else a saved computer was found on the network since: addresses
     * whose announcement carried its id. An id is public, so these are only
     * offered to the person when the saved address doesn't answer, never sent
     * the token on their own; a computer saved with `public_url` has no other.
     */
    fun foundAt(saved: Saved, found: List<Found>): List<String> {
        if (saved.fixedAddress) return emptyList()
        return found.filter { it.id == saved.id && it.address != saved.address }.map { it.address }.distinct()
    }

    /** The person chose to open the computer with [id] at [address] and its
     *  token worked there: the address it is tried at from now on. */
    fun moved(list: List<Saved>, id: String, address: String): List<Saved> =
        list.map { if (it.id == id && !it.fixedAddress) it.copy(address = address) else it }

    /** The one the app opens on by itself: the last one opened. */
    fun last(list: List<Saved>): Saved? = list.filter { it.lastOpened > 0 }.maxByOrNull { it.lastOpened }
}
