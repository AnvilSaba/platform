package io.github.valine3gdev.mcguildlink.app.discord.registry

import dev.kord.common.annotation.KordDsl
import dev.kord.core.Kord
import dev.kord.core.event.interaction.GuildButtonInteractionCreateEvent
import dev.kord.core.event.interaction.GuildModalSubmitInteractionCreateEvent
import dev.kord.core.on


/**
 * Discord ボタン・モーダルのハンドラ登録とディスパッチを行うレジストリです。
 */
@KordDsl
class InteractionRegistry(kord: Kord) {
    private val buttonInteractions = mutableSetOf<InteractionButton<*>>()
    private val modalInteractions = mutableSetOf<InteractionModal<*>>()

    init {
        kord.on<GuildButtonInteractionCreateEvent> { handleButtonInteractions() }
        kord.on<GuildModalSubmitInteractionCreateEvent> { handleModalSubmissions() }
    }

    /**
     * 受信したボタンイベントに一致するハンドラを探して実行します。
     */
    private suspend fun GuildButtonInteractionCreateEvent.handleButtonInteractions() {
        val button = buttonInteractions.find { it.id.matches(interaction.componentId) } ?: return
        button.dispatch(this, interaction.componentId)
    }

    /**
     * 解析付き custom ID を持つボタンハンドラを登録します。
     */
    fun <T, C> interactionButton(
        id: C,
        handler: InteractionButton.Handler<T>,
    ) where C : CustomId, C : CustomIdParser<T> {
        buttonInteractions += InteractionButton.parsed(id, handler)
    }

    /**
     * 固定 custom ID を持つボタンハンドラを登録します。
     */
    fun interactionButton(
        id: String,
        handler: InteractionButton.Handler<Nothing>,
    ) {
        buttonInteractions += InteractionButton(EqualsCustomId(id), handler)
    }

    /**
     * 受信したモーダル送信イベントに一致するハンドラを探して実行します。
     */
    private suspend fun GuildModalSubmitInteractionCreateEvent.handleModalSubmissions() {
        val modal = modalInteractions.find { it.id.matches(interaction.modalId) } ?: return
        modal.dispatch(this, interaction.modalId)
    }

    /**
     * 解析付き custom ID を持つモーダルハンドラを登録します。
     */
    fun <T, C> interactionModal(
        id: C,
        handler: InteractionModal.Handler<T>,
    ) where C : CustomId, C : CustomIdParser<T> {
        modalInteractions += InteractionModal.parsed(id, handler)
    }

    /**
     * 固定 custom ID を持つモーダルハンドラを登録します。
     */
    fun interactionModal(
        id: String,
        handler: InteractionModal.Handler<Nothing>,
    ) {
        modalInteractions += InteractionModal(EqualsCustomId(id), handler)
    }
}

/**
 * ボタン custom ID と実行ハンドラの組み合わせです。
 */
data class InteractionButton<T>(
    val id: CustomId,
    private val handler: Handler<T>,
    private val parser: ((String) -> T?)? = null
) {
    typealias Handler<T> = suspend GuildButtonInteractionCreateEvent.(T?) -> Unit

    /**
     * 必要なら custom ID を解析し、その結果をハンドラへ渡して実行します。
     */
    suspend fun dispatch(event: GuildButtonInteractionCreateEvent, componentId: String) {
        val data = parser?.invoke(componentId)
        handler(event, data)
    }

    companion object {
        /**
         * [CustomIdParser] を使って自動解析するボタン定義を生成します。
         */
        fun <T, C> parsed(
            id: C,
            handler: Handler<T>
        ): InteractionButton<T>
                where C : CustomId, C : CustomIdParser<T> {
            return InteractionButton(id = id, handler = handler, parser = id::parse)
        }
    }
}

/**
 * モーダル custom ID と実行ハンドラの組み合わせです。
 */
data class InteractionModal<T>(
    val id: CustomId,
    private val handler: Handler<T>,
    private val parser: ((String) -> T?)? = null
) {
    typealias Handler<T> = suspend GuildModalSubmitInteractionCreateEvent.(T?) -> Unit

    /**
     * 必要なら modal ID を解析し、その結果をハンドラへ渡して実行します。
     */
    suspend fun dispatch(event: GuildModalSubmitInteractionCreateEvent, modalId: String) {
        val data = parser?.invoke(modalId)
        handler(event, data)
    }

    companion object {
        /**
         * [CustomIdParser] を使って自動解析するモーダル定義を生成します。
         */
        fun <T, C> parsed(
            id: C,
            handler: Handler<T>
        ): InteractionModal<T>
                where C : CustomId, C : CustomIdParser<T> {
            return InteractionModal(id = id, handler = handler, parser = id::parse)
        }
    }
}
