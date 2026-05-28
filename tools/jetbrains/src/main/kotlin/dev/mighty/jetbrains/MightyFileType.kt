// File-type for *.mty files.
//
// The plugin.xml `<fileType>` extension reads `INSTANCE` from this class via
// the `fieldName="INSTANCE"` attribute. We declare it as a regular class
// (not an `object`) plus a companion-object `INSTANCE` because that's the
// shape JetBrains' XML reflection expects — and using an `object` + extra
// `INSTANCE` field trips Kotlin's CONFLICTING_JVM_DECLARATIONS checker.

package dev.mighty.jetbrains

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

class MightyFileType private constructor() : LanguageFileType(MightyLanguage) {
    override fun getName(): String = "Mighty File"

    override fun getDescription(): String = "Mighty source file"

    override fun getDefaultExtension(): String = "mty"

    override fun getIcon(): Icon = MightyIcons.File

    companion object {
        @JvmField
        val INSTANCE: MightyFileType = MightyFileType()
    }
}
