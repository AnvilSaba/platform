package io.github.valine3gdev.mcguildlink.app.service


/**
 * ホワイトリスト再生成要求を通知する抽象化です。
 */
fun interface WhitelistRefreshRequester {
    /**
     * ホワイトリストの再生成を要求します。
     */
    fun requestRefresh()
}


/**
 * ホワイトリスト再生成要求を無視する既定実装です。
 */
internal object NoopWhitelistRefreshRequester : WhitelistRefreshRequester {
    /**
     * 何も行いません。
     */
    override fun requestRefresh() = Unit
}
