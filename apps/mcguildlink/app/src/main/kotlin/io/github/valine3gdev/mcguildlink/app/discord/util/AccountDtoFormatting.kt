package io.github.valine3gdev.mcguildlink.app.discord.util

import io.github.valine3gdev.mcguildlink.app.service.dto.BlockedAccountGroupInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.DiscordAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.MinecraftAccountInfo


/**
 * Discord アカウント一覧を箇条書きへ整形します。
 */
internal fun Iterable<DiscordAccountInfo>.formatDiscordAccounts(indent: String = "- "): String =
    joinToString("\n") { account -> "$indent$account" }

/**
 * ブロックグループ内の Discord アカウント一覧を箇条書きへ整形します。
 */
internal fun BlockedAccountGroupInfo.formatDiscordAccountList(indent: String = "- "): String =
    blockedDiscordAccountInfos.formatDiscordAccounts(indent)


internal fun Iterable<MinecraftAccountInfo>.formatMinecraftAccounts(indent: String = "- "): String =
    joinToString("\n") { account -> "$indent$account" }

/**
 * Minecraft アカウント一覧を箇条書きへ整形します。
 */
internal fun BlockedAccountGroupInfo.formatMinecraftAccountList(indent: String = "- "): String =
    blockedMinecraftAccountInfos.formatMinecraftAccounts(indent)
