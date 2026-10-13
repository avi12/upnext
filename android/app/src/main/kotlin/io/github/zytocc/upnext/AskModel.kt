package io.github.zytocc.upnext

import android.os.Handler
import android.os.Looper
import androidx.lifecycle.ViewModel
import io.github.zytocc.upnext.core.InFlight
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/** What outlives [AskActivity] when the screen turns: the request in flight
 *  and the thread its calls run on. Gone when the screen closes for good. */
class AskModel : ViewModel() {
    val work: ExecutorService = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    val inFlight = InFlight(work::execute) { main.post(it) }

    override fun onCleared() {
        work.shutdownNow()
    }
}
