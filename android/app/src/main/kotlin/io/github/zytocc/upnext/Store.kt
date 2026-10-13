package io.github.zytocc.upnext

import android.content.Context
import io.github.zytocc.upnext.core.Saved
import io.github.zytocc.upnext.core.SavedComputers

/** The computers this phone was allowed on, in its private preferences,
 *  with their tokens in the [Vault]. */
class Store(context: Context) {
    private val prefs = context.getSharedPreferences("computers", Context.MODE_PRIVATE)
    private val vault = Vault(context)

    fun all(): List<Saved> = SavedComputers.decode(prefs.getString(KEY, null))

    fun find(id: String): Saved? = all().firstOrNull { it.id == id }

    fun token(id: String): String? = vault.get(id)

    /** Throws [VaultException] when the keystore can't keep the token. */
    fun put(saved: Saved, token: String) {
        vault.put(saved.id, token)
        write(SavedComputers.put(all(), saved))
    }

    fun opened(id: String) {
        val saved = find(id) ?: return
        write(SavedComputers.put(all(), saved.copy(lastOpened = System.currentTimeMillis() / 1000)))
    }

    /** The computer was reached at [address]: try it there first next time. */
    fun moved(id: String, address: String) {
        write(SavedComputers.moved(all(), id, address))
    }

    fun forget(id: String) {
        vault.remove(id)
        write(SavedComputers.forget(all(), id))
    }

    private fun write(list: List<Saved>) {
        prefs.edit().putString(KEY, SavedComputers.encode(list)).apply()
    }

    private companion object {
        const val KEY = "saved"
    }
}
