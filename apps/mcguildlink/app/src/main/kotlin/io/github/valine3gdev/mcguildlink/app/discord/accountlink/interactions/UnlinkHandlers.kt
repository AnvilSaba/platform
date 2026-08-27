package io.github.valine3gdev.mcguildlink.app.discord.accountlink.interactions

import dev.kord.core.behavior.interaction.modal
import dev.kord.core.behavior.interaction.respondEphemeral
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.UNLINK_BUTTON_ID_PREFIX
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.UNLINK_CONFIRM_CHECKBOX_ID
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.UNLINK_MODAL_ID_PREFIX
import io.github.valine3gdev.mcguildlink.app.discord.registry.InteractionRegistry
import io.github.valine3gdev.mcguildlink.app.discord.registry.createLinkedCustomId
import io.github.valine3gdev.mcguildlink.app.discord.registry.createLinkedCustomIdString
import io.github.valine3gdev.mcguildlink.app.service.AccountLinkService
import io.github.valine3gdev.mcguildlink.app.util.getLinkOrNull
import io.github.valine3gdev.mcguildlink.app.util.unlink


/**
 * 紐付け解除ボタンと確認モーダルのハンドラを登録します。
 */
context(accountLinkService: AccountLinkService)
internal fun InteractionRegistry.installUnlinkHandlers() {
    interactionButton(createLinkedCustomId(UNLINK_BUTTON_ID_PREFIX)) {
        val (userId, uuid) = it ?: error("Invalid custom ID data for unlink button: ${interaction.componentId}")
        if (userId != interaction.user.id) {
            interaction.respondEphemeral {
                content = "不正な操作です。このボタンはあなたのものではありません。"
            }
            return@interactionButton
        }

        val (_, minecraft) = accountLinkService.getLinkOrNull(interaction.user, uuid) ?: run {
            interaction.respondEphemeral {
                content = "アカウント情報を取得できませんでした。すでに解除されている可能性があります。"
            }
            return@interactionButton
        }

        interaction.modal(
            title = "アカウントの紐付け解除",
            customId = createLinkedCustomIdString(UNLINK_MODAL_ID_PREFIX, interaction.user, uuid)
        ) {
            textDisplay {
                content = """
                    本当に **${minecraft.lastKnownName}** との紐付けを解除しますか？
                    この操作は取り消せません。
                """.trimIndent()
            }

            label("内容を確認し、解除に同意します。") {
                checkbox(UNLINK_CONFIRM_CHECKBOX_ID) {
                    default = false
                }
            }
        }
    }

    interactionModal(createLinkedCustomId(UNLINK_MODAL_ID_PREFIX)) {
        val (userId, uuid) = it ?: error("Invalid custom ID data for unlink modal: ${interaction.modalId}")
        if (userId != interaction.user.id) {
            interaction.respondEphemeral {
                content = "不正な操作です。このモーダルはあなたのものではありません。"
            }
            return@interactionModal
        }

        val checkbox = interaction.checkboxes[UNLINK_CONFIRM_CHECKBOX_ID] ?: return@interactionModal
        if (!checkbox.value) {
            interaction.respondEphemeral {
                content = """
                    紐付けの解除をキャンセルしました。
                    解除する場合は、内容を確認し、チェックボックスに同意してください。
                """.trimIndent()
            }
            return@interactionModal
        }

        accountLinkService.unlink(interaction.user, uuid)
        interaction.respondEphemeral {
            content = "アカウントの紐付けを解除しました。"
        }
    }
}
