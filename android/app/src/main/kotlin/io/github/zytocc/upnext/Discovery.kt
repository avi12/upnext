package io.github.zytocc.upnext

import android.content.Context
import android.net.nsd.DiscoveryRequest
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.RequiresApi
import androidx.core.content.ContextCompat
import io.github.zytocc.upnext.core.Announcement
import io.github.zytocc.upnext.core.Found
import java.net.InetAddress

/**
 * Computers announcing UpNext on the network, through Android's own mDNS
 * (`NsdManager`), so the app needs no multicast lock and no library of its
 * own. Everything reaches [onChange] on the main thread.
 *
 * Before Android 17, or with the Nearby devices permission, it lists every
 * computer found. On Android 17 and later without it ([LocalNetwork.held])
 * an app may only see what the person chose in the system's picker
 * (`DiscoveryRequest.FLAG_SHOW_PICKER`, [pick]), so [start] lists only the
 * computers chosen there before (`FLAG_USER_APPROVED_ONLY`), with no prompt.
 * Either way each computer found is looked up once, as before.
 */
class Discovery(private val context: Context, private val onChange: (List<Found>) -> Unit) {
    private val nsd = context.getSystemService(NsdManager::class.java)
    private val executor = ContextCompat.getMainExecutor(context)
    private val main = Handler(Looper.getMainLooper())

    /** By service name, which is how `NsdManager` reports one lost. */
    private val found = LinkedHashMap<String, Found>()
    private var listener: NsdManager.DiscoveryListener? = null

    /** Android 14 and later: one callback per service, kept up to date. */
    private val watching = HashMap<String, NsdManager.ServiceInfoCallback>()

    /** Before Android 14: one resolve at a time, so the rest wait here. */
    private val queue = ArrayDeque<NsdServiceInfo>()
    private var resolving = false

    /** Whether computers are found through the system's picker ([pick])
     *  rather than listed as they announce themselves. */
    val picks: Boolean get() = LocalNetwork.held(context)

    /** Whether the picker is showing. The picker may be a screen of its
     *  own, so the screen behind keeps discovery running meanwhile. */
    var picking = false
        private set

    fun start() {
        if (listener != null || nsd == null) return
        listen(if (picks) Mode.CHOSEN_BEFORE else Mode.ALL)
    }

    /**
     * Android 17 and later: the system's picker, where the person chooses
     * one computer among those announcing UpNext. The chosen one is looked
     * up like any other and stays listed; the picker remembers it, so the
     * next [start] finds it again without asking.
     */
    fun pick() {
        if (nsd == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.CINNAMON_BUN) return
        listener?.let { l ->
            listener = null
            runCatching { nsd.stopServiceDiscovery(l) }
        }
        listen(Mode.PICKER)
    }

    private enum class Mode { ALL, CHOSEN_BEFORE, PICKER }

    private fun listen(mode: Mode) {
        val nsd = nsd ?: return
        val l = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {}

            override fun onDiscoveryStopped(serviceType: String) {
                // The picker stops itself once a computer is chosen or it
                // is cancelled; then the quiet listing comes back.
                main.post {
                    if (listener === this && mode == Mode.PICKER) {
                        listener = null
                        picking = false
                        start()
                    }
                }
            }

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                main.post {
                    if (listener === this) {
                        listener = null
                        if (mode == Mode.PICKER) {
                            picking = false
                            start()
                        }
                    }
                }
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                main.post { if (listener === this) resolve(serviceInfo) }
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                main.post { if (listener === this) lost(serviceInfo.serviceName) }
            }
        }
        listener = l
        picking = mode == Mode.PICKER
        try {
            if (mode != Mode.ALL && Build.VERSION.SDK_INT >= Build.VERSION_CODES.CINNAMON_BUN) {
                discoverChosen(nsd, mode == Mode.PICKER, l)
            } else {
                nsd.discoverServices(Announcement.SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, l)
            }
        } catch (e: IllegalArgumentException) {
            listener = null
            picking = false
        }
    }

    @RequiresApi(Build.VERSION_CODES.CINNAMON_BUN)
    private fun discoverChosen(nsd: NsdManager, show: Boolean, l: NsdManager.DiscoveryListener) {
        val flags = if (show) DiscoveryRequest.FLAG_SHOW_PICKER else DiscoveryRequest.FLAG_USER_APPROVED_ONLY
        val request = DiscoveryRequest.Builder(Announcement.SERVICE_TYPE).setFlags(flags).build()
        nsd.discoverServices(request, executor, l)
    }

    fun stop() {
        picking = false
        val l = listener ?: return
        listener = null
        runCatching { nsd?.stopServiceDiscovery(l) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            watching.values.forEach { cb -> runCatching { nsd?.unregisterServiceInfoCallback(cb) } }
        }
        watching.clear()
        queue.clear()
        resolving = false
        found.clear()
    }

    private fun resolve(info: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            watch(info)
        } else {
            if (queue.none { it.serviceName == info.serviceName }) queue.addLast(info)
            next()
        }
    }

    @RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
    private fun watch(info: NsdServiceInfo) {
        val name = info.serviceName
        if (watching.containsKey(name)) return
        val cb = object : NsdManager.ServiceInfoCallback {
            override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                watching.remove(name)
            }

            override fun onServiceUpdated(serviceInfo: NsdServiceInfo) {
                if (watching[name] === this) update(serviceInfo, serviceInfo.hostAddresses)
            }

            override fun onServiceLost() {
                if (watching[name] === this) lost(name)
            }

            override fun onServiceInfoCallbackUnregistered() {}
        }
        watching[name] = cb
        try {
            nsd?.registerServiceInfoCallback(info, executor, cb)
        } catch (e: IllegalArgumentException) {
            watching.remove(name)
        }
    }

    @Suppress("DEPRECATION")
    private fun next() {
        if (resolving || listener == null) return
        val info = queue.removeFirstOrNull() ?: return
        resolving = true
        val l = listener
        nsd?.resolveService(
            info,
            object : NsdManager.ResolveListener {
                override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                    main.post {
                        resolving = false
                        next()
                    }
                }

                override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                    main.post {
                        resolving = false
                        if (listener === l) update(serviceInfo, listOfNotNull(serviceInfo.host))
                        next()
                    }
                }
            },
        )
    }

    private fun update(info: NsdServiceInfo, addresses: List<InetAddress>) {
        val computer = Announcement.read(info.attributes, info.serviceName, addresses, info.port) ?: return
        if (found[info.serviceName] == computer) return
        found[info.serviceName] = computer
        onChange(found.values.toList())
    }

    private fun lost(name: String) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            watching.remove(name)?.let { cb -> runCatching { nsd?.unregisterServiceInfoCallback(cb) } }
        }
        if (found.remove(name) != null) onChange(found.values.toList())
    }
}
