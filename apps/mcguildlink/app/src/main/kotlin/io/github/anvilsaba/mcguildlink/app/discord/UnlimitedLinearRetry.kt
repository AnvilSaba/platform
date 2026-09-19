package io.github.anvilsaba.mcguildlink.app.discord

import dev.kord.gateway.retry.Retry
import kotlinx.coroutines.delay
import java.util.concurrent.atomic.AtomicReference
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds

/**
 * Discord Gateway への再接続を成功するまで継続します。
 *
 * 再試行間隔は線形に増加し、[maxBackoff] で頭打ちになります。
 */
internal class UnlimitedLinearRetry(
    private val firstBackoff: Duration = 2.seconds,
    private val maxBackoff: Duration = 20.seconds,
) : Retry {
    init {
        require(firstBackoff.isPositive()) { "firstBackoff must be positive" }
        require(maxBackoff >= firstBackoff) {
            "maxBackoff must be greater than or equal to firstBackoff"
        }
    }

    private val nextBackoff = AtomicReference(firstBackoff)

    override val hasNext: Boolean
        get() = true

    override fun reset() {
        nextBackoff.set(firstBackoff)
    }

    override suspend fun retry() {
        val backoff = nextBackoff.getAndUpdate { current ->
            minOf(current + firstBackoff, maxBackoff)
        }
        delay(backoff)
    }
}
