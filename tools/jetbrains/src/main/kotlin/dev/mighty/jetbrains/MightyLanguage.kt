// Mighty language registration.
//
// The IntelliJ Platform identifies a language by a single Language object.
// We keep ours minimal — the heavy lifting (parsing, highlighting,
// completion) happens via LSP, so we don't ship a custom PSI parser.

package dev.mighty.jetbrains

import com.intellij.lang.Language

object MightyLanguage : Language("Mighty") {
    private fun readResolve(): Any = MightyLanguage

    override fun isCaseSensitive(): Boolean = true
}
