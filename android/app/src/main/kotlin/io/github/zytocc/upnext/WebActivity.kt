package io.github.zytocc.upnext

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.view.View
import android.webkit.CookieManager
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import io.github.zytocc.upnext.core.Saved

/**
 * UpNext itself: the computer's own web UI in a full-screen WebView. The
 * token goes in as the paired-browser cookie, a session cookie set just
 * before the first page loads and cleared when the person leaves the
 * computer, so the only copy at rest is the sealed one in the [Vault].
 */
class WebActivity : ComponentActivity() {
    private lateinit var web: WebView
    private lateinit var problem: View
    private lateinit var saved: Saved
    private lateinit var origin: Uri

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        val store = Store(this)
        val id = intent.getStringExtra(EXTRA_ID)
        val found = id?.let(store::find)
        val token = id?.let(store::token)
        if (found == null || token == null) {
            finish()
            return
        }
        saved = found
        origin = Uri.parse(saved.address)
        store.opened(saved.id)

        setContentView(R.layout.activity_web)
        padForSystemBars(findViewById(R.id.root), keyboard = true)
        web = findViewById(R.id.web)
        problem = findViewById(R.id.problem)
        findViewById<Button>(R.id.retry).setOnClickListener { load(token) }
        findViewById<Button>(R.id.computers).setOnClickListener { finish() }

        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) WebView.setWebContentsDebuggingEnabled(true)
        web.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            setSupportMultipleWindows(false)
        }
        CookieManager.getInstance().apply {
            setAcceptCookie(true)
            setAcceptThirdPartyCookies(web, false)
        }
        web.webChromeClient = WebChromeClient()
        web.webViewClient = Client()
        web.isFocusable = true
        web.isFocusableInTouchMode = true

        onBackPressedDispatcher.addCallback(
            this,
            object : OnBackPressedCallback(true) {
                override fun handleOnBackPressed() {
                    if (problem.visibility != View.VISIBLE && web.canGoBack()) web.goBack() else finish()
                }
            },
        )

        // Always from the start: after the process was stopped, the session
        // cookie is gone, so a restored page would only be refused.
        load(token)
    }

    override fun onResume() {
        super.onResume()
        if (::web.isInitialized) web.onResume()
    }

    override fun onPause() {
        if (::web.isInitialized) web.onPause()
        super.onPause()
    }

    override fun onDestroy() {
        if (::web.isInitialized) {
            web.destroy()
            if (isFinishing) CookieManager.getInstance().removeAllCookies(null)
        }
        super.onDestroy()
    }

    /** Set the cookie, then open UpNext: big screen on a TV, as the plan has
     *  it, since a TV is driven from across the room with a remote or pad. */
    private fun load(token: String) {
        problem.visibility = View.GONE
        web.visibility = View.VISIBLE
        val cookies = CookieManager.getInstance()
        cookies.removeAllCookies {
            val secure = if (origin.scheme == "https") "; Secure" else ""
            cookies.setCookie(saved.address, "upnext_device=$token; Path=/; HttpOnly; SameSite=Strict$secure") {
                val start = if (Device.isTv(this)) "/?big=1" else "/"
                web.loadUrl(saved.address + start)
                web.requestFocus()
            }
        }
    }

    private fun say(message: String) = Toast.makeText(this, message, Toast.LENGTH_LONG).show()

    private fun sameOrigin(url: Uri): Boolean =
        url.scheme == origin.scheme && url.host.equals(origin.host, ignoreCase = true) && port(url) == port(origin)

    private fun port(url: Uri): Int = if (url.port != -1) url.port else if (url.scheme == "https") 443 else 80

    private inner class Client : WebViewClient() {
        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            val url = request.url
            // UpNext's own pages, and Steam's sign-in page, which sends the
            // browser back to the computer, stay here.
            val steamSignIn = url.scheme == "https" && url.host == "steamcommunity.com" && url.path.orEmpty().startsWith("/openid")
            if (sameOrigin(url) || steamSignIn) return false
            // Only web links go to another app. Play's steam:// and heroic://
            // links, and any other scheme, have nowhere to go on a phone: the
            // game starts on the computer.
            if (url.scheme != "http" && url.scheme != "https") {
                say(getString(R.string.play_on_computer, saved.name))
                return true
            }
            try {
                startActivity(Intent(Intent.ACTION_VIEW, url).addCategory(Intent.CATEGORY_BROWSABLE))
            } catch (e: ActivityNotFoundException) {
                say(getString(R.string.no_browser))
            } catch (e: RuntimeException) {
                say(getString(R.string.link_failed))
            }
            return true
        }

        override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
            if (!request.isForMainFrame) return
            web.visibility = View.INVISIBLE
            problem.visibility = View.VISIBLE
            findViewById<TextView>(R.id.problem_text).text = getString(R.string.page_failed, saved.name, error.description)
        }
    }

    companion object {
        private const val EXTRA_ID = "id"

        fun intent(context: Context, id: String): Intent = Intent(context, WebActivity::class.java).putExtra(EXTRA_ID, id)
    }
}
