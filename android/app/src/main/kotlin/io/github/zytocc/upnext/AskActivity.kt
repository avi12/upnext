package io.github.zytocc.upnext

import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.isVisible
import androidx.lifecycle.ViewModelProvider
import io.github.zytocc.upnext.core.Allowed
import io.github.zytocc.upnext.core.Asked
import io.github.zytocc.upnext.core.InFlight
import io.github.zytocc.upnext.core.PairClient
import io.github.zytocc.upnext.core.Saved
import io.github.zytocc.upnext.core.Status
import io.github.zytocc.upnext.core.UpNextException
import java.net.URI

/**
 * Asking a computer to allow this device (the one-tap
 * approval): the computer shows the request with a four-digit number, the
 * device shows the same number while it waits, and someone at the computer
 * taps Allow. "Type a code instead" pairs with Settings' six-digit code,
 * and a QR code that carries the code pairs with it straight away.
 */
class AskActivity : ComponentActivity() {
    private lateinit var address: String

    /** The install id the computer was found with, when it was found. */
    private var expectedId: String? = null
    private lateinit var title: TextView
    private lateinit var matchCode: TextView
    private lateinit var status: TextView
    private lateinit var askAgain: Button
    private lateinit var codeField: EditText
    private val client = PairClient()
    private val store by lazy { Store(this) }
    private val model by lazy { ViewModelProvider(this)[AskModel::class.java] }
    private val main = Handler(Looper.getMainLooper())
    private var asked: Asked? = null
    private var done = false

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_ask)
        padForSystemBars(findViewById(R.id.root))
        address = intent.getStringExtra(EXTRA_ADDRESS) ?: return finish()
        expectedId = intent.getStringExtra(EXTRA_ID)
        title = findViewById(R.id.title)
        matchCode = findViewById(R.id.match_code)
        status = findViewById(R.id.status)
        askAgain = findViewById(R.id.ask_again)
        codeField = findViewById(R.id.code)
        title.text = getString(R.string.asking_title, host(address))
        askAgain.setOnClickListener { ask() }
        findViewById<Button>(R.id.cancel).setOnClickListener { finish() }
        findViewById<Button>(R.id.pair_code).setOnClickListener { typedCode() }
        codeField.setOnEditorActionListener { _, action, _ ->
            if (action == EditorInfo.IME_ACTION_DONE) typedCode()
            action == EditorInfo.IME_ACTION_DONE
        }

        // A turn of the screen makes this screen anew: it takes the answer
        // to the request still in flight, keeps waiting on the request the
        // computer answered with, or keeps the answer it showed, rather than
        // asking again, which would show the computer a new number or send
        // a single-use code twice.
        val inFlight = model.inFlight.kind
        val waiting = savedInstanceState?.getStringArray(STATE_ASKED)?.let(Asked::fromState)
        val stoppedWith = savedInstanceState?.getCharSequence(STATE_STOPPED)
        val code = intent.getStringExtra(EXTRA_CODE)
        when {
            inFlight != null -> working(inFlight)
            waiting != null -> waitFor(waiting)
            stoppedWith != null -> {
                savedInstanceState.getCharSequence(STATE_TITLE)?.let { title.text = it }
                stopped(stoppedWith.toString())
            }
            code != null -> pairWithCode(code)
            else -> ask()
        }
        model.inFlight.attach(::answered)
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        // Nothing to keep when onCreate finished at once, with no address.
        if (!::askAgain.isInitialized) return
        // The request's secret only collects this device's answer, and only
        // until the request expires a few minutes after it was made.
        asked?.let { outState.putStringArray(STATE_ASKED, it.toState()) }
        if (askAgain.isVisible) {
            outState.putCharSequence(STATE_STOPPED, status.text)
            outState.putCharSequence(STATE_TITLE, title.text)
        }
    }

    override fun onDestroy() {
        done = true
        main.removeCallbacksAndMessages(null)
        if (::address.isInitialized) model.inFlight.detach()
        super.onDestroy()
    }

    private fun ask() {
        working(InFlight.Kind.ASK)
        val name = Device.name(this)
        val kind = Device.kind(this)
        // Locals, so the call on the worker holds no reference to this
        // screen, which a turn of the screen replaces.
        val (pair, at, id) = Triple(client, address, expectedId)
        model.inFlight.start(InFlight.Kind.ASK) { pair.ask(at, name, kind, id) }
    }

    /** A request is on its way: no number yet. */
    private fun working(kind: InFlight.Kind) {
        main.removeCallbacksAndMessages(null)
        asked = null
        askAgain.visibility = View.GONE
        matchCode.visibility = View.INVISIBLE
        status.text = when (kind) {
            InFlight.Kind.ASK -> getString(R.string.asking, host(address))
            InFlight.Kind.CODE -> getString(R.string.pairing_with_code)
        }
    }

    /** The answer to the request in flight, whichever screen made it. */
    private fun answered(kind: InFlight.Kind, result: Result<Any>) {
        if (done || isFinishing) return
        when (kind) {
            InFlight.Kind.ASK -> result.fold(onSuccess = { waitFor(it as Asked) }, onFailure = ::failed)
            InFlight.Kind.CODE -> result.fold(onSuccess = { allowed(it as Allowed) }, onFailure = ::failed)
        }
    }

    /** Show the request's number and wait for the computer's answer. */
    private fun waitFor(request: Asked) {
        asked = request
        askAgain.visibility = View.GONE
        title.text = getString(R.string.asking_title, request.computer)
        matchCode.text = request.matchCode
        matchCode.visibility = View.VISIBLE
        status.text = getString(R.string.waiting_for_allow, request.computer, Device.name(this), request.matchCode)
        main.postDelayed({ collect(request) }, POLL_MILLIS)
    }

    private fun collect(request: Asked) {
        if (asked !== request) return
        model.work.execute {
            val result = runCatching { client.collect(address, request) }
            ui {
                if (asked !== request) return@ui
                val answer = result.getOrElse {
                    failed(it)
                    return@ui
                }
                when (answer.status) {
                    Status.WAITING -> main.postDelayed({ collect(request) }, POLL_MILLIS)
                    Status.ALLOWED -> answer.allowed?.let(::allowed)
                    Status.DENIED -> stopped(getString(R.string.denied, request.computer))
                    Status.EXPIRED -> stopped(getString(R.string.expired))
                }
            }
        }
    }

    private fun typedCode() {
        val code = codeField.text.toString().filter { !it.isWhitespace() }
        if (code.length != 6 || !code.all(Char::isDigit)) {
            codeField.error = getString(R.string.code_is_six_digits)
            return
        }
        pairWithCode(code)
    }

    private fun pairWithCode(code: String) {
        working(InFlight.Kind.CODE)
        val name = Device.name(this)
        val (pair, at, id) = Triple(client, address, expectedId)
        model.inFlight.start(InFlight.Kind.CODE) { pair.pairWithCode(at, code, name, id) }
    }

    /** Allowed: keep the token sealed, and open UpNext there. */
    private fun allowed(a: Allowed) {
        val at = a.address ?: address
        val id = a.computerId
        val saved = Saved(
            id = id,
            name = a.computer ?: host(at),
            address = at,
            tokenId = a.tokenId,
            lastOpened = System.currentTimeMillis() / 1000,
            fixedAddress = a.address != null,
        )
        try {
            store.put(saved, a.token)
        } catch (e: VaultException) {
            stopped(getString(R.string.keystore_failed))
            return
        }
        startActivity(WebActivity.intent(this, id))
        finish()
    }

    private fun failed(e: Throwable) {
        val reason = (e as? UpNextException)?.message ?: getString(R.string.something_went_wrong)
        stopped(reason)
    }

    private fun stopped(message: String) {
        asked = null
        matchCode.visibility = View.INVISIBLE
        status.text = message
        askAgain.visibility = View.VISIBLE
    }

    private fun ui(block: () -> Unit) = runOnUiThread { if (!done && !isFinishing) block() }

    private fun host(address: String): String = runCatching { URI(address).host }.getOrNull() ?: address

    companion object {
        private const val EXTRA_ADDRESS = "address"
        private const val EXTRA_CODE = "code"
        private const val EXTRA_ID = "id"
        private const val STATE_ASKED = "asked"
        private const val STATE_STOPPED = "stopped"
        private const val STATE_TITLE = "title"

        /** The collect is asked every two seconds, as the computer's own dialog polls. */
        private const val POLL_MILLIS = 2000L

        fun intent(context: Context, address: String, code: String?, id: String? = null): Intent =
            Intent(context, AskActivity::class.java).putExtra(EXTRA_ADDRESS, address).putExtra(EXTRA_CODE, code).putExtra(EXTRA_ID, id)
    }
}
