package io.github.zytocc.upnext

import android.app.AlertDialog
import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.text.InputType
import android.view.LayoutInflater
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import io.github.zytocc.upnext.core.Addresses
import io.github.zytocc.upnext.core.Found
import io.github.zytocc.upnext.core.PairClient
import io.github.zytocc.upnext.core.Saved
import io.github.zytocc.upnext.core.SavedComputers
import io.github.zytocc.upnext.core.TokenState
import io.github.zytocc.upnext.core.listComputers
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * The first screen: the computers found on the network and those allowed
 * before, "Scan the QR code" and "Add by address". Opening the app goes
 * straight to the computer opened last when it still answers.
 */
class ComputersActivity : ComponentActivity() {
    private lateinit var store: Store
    private lateinit var discovery: Discovery
    private lateinit var list: LinearLayout
    private lateinit var status: TextView
    private val client = PairClient()
    private val work: ExecutorService = Executors.newSingleThreadExecutor()
    private var found: List<Found> = emptyList()

    private val scan = registerForActivityResult(ScanContract()) { result ->
        result.contents?.let(::scanned)
    }

    /**
     * The address (and the code, if any) to ask at once the person has
     * answered the Nearby devices prompt. Plain values kept in the saved
     * state, not a closure: turning the phone while the prompt shows makes
     * this screen anew, and the answer comes back to the new one.
     */
    private var askAfterPrompt: String? = null
    private var codeAfterPrompt: String? = null

    private val localNetwork = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) {
            // Every computer on the network can be listed now.
            discovery.stop()
            discovery.start()
            render()
        }
        val address = askAfterPrompt
        val code = codeAfterPrompt
        askAfterPrompt = null
        codeAfterPrompt = null
        if (address != null) ask(address, code)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        askAfterPrompt = savedInstanceState?.getString(STATE_ASK_ADDRESS)
        codeAfterPrompt = savedInstanceState?.getString(STATE_ASK_CODE)
        setContentView(R.layout.activity_computers)
        padForSystemBars(findViewById(R.id.root))
        store = Store(this)
        list = findViewById(R.id.computers)
        status = findViewById(R.id.status)
        discovery = Discovery(this) { now ->
            found = now
            render()
        }
        findViewById<Button>(R.id.scan).setOnClickListener {
            scan.launch(
                ScanOptions()
                    .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                    .setPrompt(getString(R.string.scan_prompt))
                    .setBeepEnabled(false)
                    .setOrientationLocked(false),
            )
        }
        findViewById<Button>(R.id.add_address).setOnClickListener { addByAddress() }
        findViewById<Button>(R.id.find).setOnClickListener { discovery.pick() }
        findViewById<Button>(R.id.licences).setOnClickListener { showLicences() }

        val launched = savedInstanceState == null && intent?.action == Intent.ACTION_MAIN
        if (launched) SavedComputers.last(store.all())?.let { open(it, onLaunch = true) }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString(STATE_ASK_ADDRESS, askAfterPrompt)
        outState.putString(STATE_ASK_CODE, codeAfterPrompt)
    }

    override fun onStart() {
        super.onStart()
        discovery.start()
        render()
    }

    override fun onStop() {
        // The system's picker may cover this screen; it needs its discovery.
        if (!discovery.picking) {
            discovery.stop()
            found = emptyList()
        }
        super.onStop()
    }

    override fun onDestroy() {
        work.shutdownNow()
        super.onDestroy()
    }

    private fun render() {
        list.removeAllViews()
        val rows = listComputers(store.all(), found)
        for (row in rows) {
            val view = LayoutInflater.from(this).inflate(R.layout.item_computer, list, false)
            view.findViewById<TextView>(R.id.name).text = row.name
            val details = buildList {
                add(row.address)
                row.found?.version?.let { add(getString(R.string.computer_version, it)) }
                when {
                    row.saved != null && row.found == null -> add(getString(R.string.computer_not_seen))
                    row.saved != null -> add(getString(R.string.computer_allowed))
                    else -> add(getString(R.string.computer_new))
                }
            }
            view.findViewById<TextView>(R.id.details).text = details.joinToString(" · ")
            val forget = view.findViewById<Button>(R.id.forget)
            val saved = row.saved
            if (saved != null) {
                forget.visibility = View.VISIBLE
                forget.setOnClickListener { confirmForget(saved) }
                view.setOnClickListener { open(saved, onLaunch = false) }
            } else {
                forget.visibility = View.GONE
                view.setOnClickListener { ask(row.address, code = null, id = row.id) }
            }
            list.addView(view)
        }
        val picks = discovery.picks
        findViewById<Button>(R.id.find).visibility = if (picks) View.VISIBLE else View.GONE
        status.text = getString(
            when {
                found.isNotEmpty() -> R.string.found_hint
                picks -> R.string.find_hint
                else -> R.string.looking
            },
        )
    }

    /** Open a saved computer, after checking its token still works there. */
    private fun open(saved: Saved, onLaunch: Boolean) {
        val token = store.token(saved.id)
        if (token == null) {
            // The keystore's key is gone (a restore onto another phone).
            store.forget(saved.id)
            ask(saved.address, code = null, id = saved.id)
            return
        }
        status.text = getString(R.string.opening, saved.name)
        val elsewhere = SavedComputers.foundAt(saved, found)
        work.execute {
            val reached = client.reach(saved.address, elsewhere, token, saved.id)
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                val moved = reached.elsewhere
                when (val state = reached.state) {
                    TokenState.Works -> startActivity(WebActivity.intent(this, saved.id))
                    TokenState.Revoked -> {
                        store.forget(saved.id)
                        render()
                        Toast.makeText(this, getString(R.string.revoked_there, saved.name), Toast.LENGTH_LONG).show()
                        ask(saved.address, code = null, id = saved.id)
                    }
                    else -> {
                        render()
                        if (moved != null) {
                            confirmMove(saved, token, moved)
                            return@runOnUiThread
                        }
                        val message = when (state) {
                            is TokenState.Unreachable -> getString(R.string.unreachable, saved.name, state.reason) + localNetworkHint()
                            else -> getString(R.string.changed_there, saved.name, saved.address)
                        }
                        if (onLaunch) {
                            status.text = message
                        } else {
                            AlertDialog.Builder(this)
                                .setMessage(message)
                                .setPositiveButton(R.string.try_again) { _, _ -> open(saved, onLaunch = false) }
                                .setNegativeButton(android.R.string.cancel, null)
                                .show()
                        }
                    }
                }
            }
        }
    }

    /**
     * The saved address doesn't answer, but the computer's id answers from
     * [address]. Anyone on the network can announce an id, so the token goes
     * there only after the person says so, and the address is kept only once
     * the token works there.
     */
    private fun confirmMove(saved: Saved, token: String, address: String) {
        status.text = getString(R.string.found_elsewhere_short, saved.name)
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.found_elsewhere_title, saved.name))
            .setMessage(getString(R.string.found_elsewhere, saved.name, saved.address, address))
            .setPositiveButton(R.string.use_new_address) { _, _ ->
                status.text = getString(R.string.opening, saved.name)
                work.execute {
                    val state = client.check(address, token, saved.id)
                    runOnUiThread {
                        if (isFinishing || isDestroyed) return@runOnUiThread
                        if (state == TokenState.Works) {
                            store.moved(saved.id, address)
                            render()
                            startActivity(WebActivity.intent(this, saved.id))
                        } else {
                            val reason = (state as? TokenState.Unreachable)?.reason ?: getString(R.string.not_allowed_there)
                            status.text = getString(R.string.unreachable, saved.name, reason)
                        }
                    }
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    /** Ask a computer to allow this device; [id] is the install id it was
     *  found with, which the address must still answer with. */
    private fun ask(address: String, code: String?, id: String? = null) {
        startActivity(AskActivity.intent(this, address, code, id))
    }

    /**
     * Ask at an address typed or scanned. On Android 17 and later, one that
     * may be on this network needs the Nearby devices permission, unless the
     * picker found it, so the system's prompt comes first. The asking goes
     * ahead whatever the answer: an address through a VPN such as Tailscale
     * needs no permission, and a refused one says why it can't be reached.
     */
    private fun askTyped(address: String, code: String?) {
        if (LocalNetwork.held(this) && Addresses.mayBeOnThisNetwork(address)) {
            askAfterPrompt = address
            codeAfterPrompt = code
            localNetwork.launch(LocalNetwork.PERMISSION)
        } else {
            ask(address, code)
        }
    }

    /** Android 17 and later, without the permission: why a computer on this
     *  network may not answer, and what to do about it. */
    private fun localNetworkHint(): String = if (LocalNetwork.held(this)) " " + getString(R.string.local_network_hint) else ""

    private fun scanned(text: String) {
        val qr = Addresses.fromQr(text)
        if (qr == null) {
            Toast.makeText(this, R.string.not_an_upnext_code, Toast.LENGTH_LONG).show()
            return
        }
        askTyped(qr.address, qr.code)
    }

    private fun addByAddress() {
        val field = EditText(this).apply {
            hint = getString(R.string.address_hint)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            imeOptions = EditorInfo.IME_ACTION_GO
            isSingleLine = true
        }
        val padding = (resources.displayMetrics.density * 20).toInt()
        val frame = LinearLayout(this).apply {
            setPadding(padding, padding / 2, padding, 0)
            addView(field, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
        }
        val dialog = AlertDialog.Builder(this)
            .setTitle(R.string.add_by_address)
            .setMessage(R.string.add_by_address_help)
            .setView(frame)
            .setPositiveButton(R.string.ask_to_be_allowed, null)
            .setNegativeButton(android.R.string.cancel, null)
            .create()
        val go = {
            val address = Addresses.fromTyped(field.text.toString())
            if (address == null) {
                field.error = getString(R.string.not_an_address)
            } else {
                dialog.dismiss()
                askTyped(address, code = null)
            }
        }
        field.setOnEditorActionListener { _, action, _ ->
            if (action == EditorInfo.IME_ACTION_GO) go()
            action == EditorInfo.IME_ACTION_GO
        }
        dialog.setOnShowListener { dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener { go() } }
        dialog.show()
    }

    /**
     * The app's own libraries and their licence, from the asset
     * contrib/notices/generate.mjs writes. The computer's UpNext lists what
     * it is built with in Settings › About.
     */
    private fun showLicences() {
        val text = assets.open("third_party_notices.txt").bufferedReader().use { it.readText() }
        AlertDialog.Builder(this)
            .setTitle(R.string.licences)
            .setMessage(text)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private fun confirmForget(saved: Saved) {
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.forget_title, saved.name))
            .setMessage(getString(R.string.forget_message, saved.name))
            .setPositiveButton(R.string.forget) { _, _ -> forget(saved) }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    /** Forget a computer here, and ask it to revoke this phone's token when
     *  it can be reached; otherwise say where to revoke it. */
    private fun forget(saved: Saved) {
        val token = store.token(saved.id)
        val tokenId = saved.tokenId
        store.forget(saved.id)
        render()
        val app = applicationContext
        work.execute {
            val revoked = token != null && tokenId != null && client.revoke(saved.address, token, tokenId, saved.id)
            runOnUiThread {
                val message = if (revoked) R.string.forgotten_and_revoked else R.string.forgotten_here
                Toast.makeText(app, app.getString(message, saved.name), Toast.LENGTH_LONG).show()
            }
        }
    }

    private companion object {
        const val STATE_ASK_ADDRESS = "ask_after_prompt_address"
        const val STATE_ASK_CODE = "ask_after_prompt_code"
    }
}
