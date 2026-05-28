// Color customisation for Mighty tokens.
//
// Even though the heavy lifting (semantic highlighting) goes through LSP,
// JetBrains expects every language to ship a ColorSettingsPage so that users
// can rebind highlight colors in Settings > Editor > Color Scheme.
//
// We map a handful of "semantic" categories onto the platform-default
// `DefaultLanguageHighlighterColors` attributes; the LSP server's
// semantic-token classifier will set these on individual tokens at runtime.

package dev.mighty.jetbrains

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.PlainSyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import javax.swing.Icon

class MightyColorSettingsPage : ColorSettingsPage {
    override fun getIcon(): Icon = MightyIcons.File

    override fun getHighlighter(): SyntaxHighlighter = PlainSyntaxHighlighter()

    override fun getDemoText(): String = """
        // SPDX-License-Identifier: MIT
        module billing {
            agent Refund {
                intent: "Issue refund if order eligible",
                inputs { order_id: String }
                outputs { ok: Bool }
                body {
                    let order = lookup(order_id)
                    return order.refundable && refund(order)
                }
            }
        }
    """.trimIndent()

    override fun getAdditionalHighlightingTagToDescriptorMap(): Map<String, TextAttributesKey>? = null

    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = DESCRIPTORS

    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY

    override fun getDisplayName(): String = "Mighty"

    companion object {
        // Keys are referenced by `DefaultLanguageHighlighterColors.*` so that
        // they pick up the user's existing colour scheme by default but can
        // still be re-bound per Mighty token type.
        private val KEYWORD = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_KEYWORD",
            DefaultLanguageHighlighterColors.KEYWORD,
        )
        private val IDENTIFIER = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_IDENTIFIER",
            DefaultLanguageHighlighterColors.IDENTIFIER,
        )
        private val STRING = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_STRING",
            DefaultLanguageHighlighterColors.STRING,
        )
        private val NUMBER = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_NUMBER",
            DefaultLanguageHighlighterColors.NUMBER,
        )
        private val LINE_COMMENT = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_LINE_COMMENT",
            DefaultLanguageHighlighterColors.LINE_COMMENT,
        )
        private val DOC_COMMENT = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_DOC_COMMENT",
            DefaultLanguageHighlighterColors.DOC_COMMENT,
        )
        private val FUNCTION = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_FUNCTION",
            DefaultLanguageHighlighterColors.FUNCTION_DECLARATION,
        )
        private val TYPE = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_TYPE",
            DefaultLanguageHighlighterColors.CLASS_NAME,
        )
        private val PARAMETER = TextAttributesKey.createTextAttributesKey(
            "MIGHTY_PARAMETER",
            DefaultLanguageHighlighterColors.PARAMETER,
        )

        private val DESCRIPTORS = arrayOf(
            AttributesDescriptor("Keyword", KEYWORD),
            AttributesDescriptor("Identifier", IDENTIFIER),
            AttributesDescriptor("String", STRING),
            AttributesDescriptor("Number", NUMBER),
            AttributesDescriptor("Comment//Line", LINE_COMMENT),
            AttributesDescriptor("Comment//Doc", DOC_COMMENT),
            AttributesDescriptor("Function", FUNCTION),
            AttributesDescriptor("Type", TYPE),
            AttributesDescriptor("Parameter", PARAMETER),
        )
    }
}
