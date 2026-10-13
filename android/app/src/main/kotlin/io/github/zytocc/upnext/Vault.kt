package io.github.zytocc.upnext

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The computers' tokens, sealed with an AES key that lives in Android's
 * keystore and never leaves it. The app's preferences hold only the sealed
 * bytes, so a copy of its files without the phone's keystore opens nothing.
 * Each token is bound to its computer's id, so one can't be moved to
 * another computer's slot.
 */
class Vault(context: Context) {
    private val prefs = context.getSharedPreferences("vault", Context.MODE_PRIVATE)

    /** Seal and keep [token]. Throws [VaultException] when the keystore
     *  can't make or use its key, as on some devices without a working one;
     *  the token is never kept unsealed instead. */
    fun put(id: String, token: String) {
        val sealed = try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            cipher.updateAAD(id.toByteArray(Charsets.UTF_8))
            cipher.iv + cipher.doFinal(token.toByteArray(Charsets.UTF_8))
        } catch (e: GeneralSecurityException) {
            throw VaultException(e)
        } catch (e: IOException) {
            throw VaultException(e)
        } catch (e: ProviderException) {
            throw VaultException(e)
        } catch (e: IllegalStateException) {
            throw VaultException(e)
        }
        prefs.edit().putString(id, Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
    }

    /** The token, or null when there is none or it can't be opened (the
     *  keystore's key was lost, as after a restore onto another phone). */
    fun get(id: String): String? {
        val text = prefs.getString(id, null) ?: return null
        return try {
            val sealed = Base64.decode(text, Base64.NO_WRAP)
            if (sealed.size <= IV_BYTES) return null
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, sealed, 0, IV_BYTES))
            cipher.updateAAD(id.toByteArray(Charsets.UTF_8))
            String(cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES), Charsets.UTF_8)
        } catch (e: GeneralSecurityException) {
            null
        } catch (e: IOException) {
            null
        } catch (e: ProviderException) {
            null
        } catch (e: IllegalArgumentException) {
            null
        } catch (e: IllegalStateException) {
            null
        }
    }

    fun remove(id: String) {
        prefs.edit().remove(id).apply()
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "upnext-tokens"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val TAG_BITS = 128
    }
}

/** The keystore couldn't seal a token. */
class VaultException(cause: Throwable) : Exception(cause.message, cause)
