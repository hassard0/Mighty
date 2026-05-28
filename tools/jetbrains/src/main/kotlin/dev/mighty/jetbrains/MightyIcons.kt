// Centralised icon handles, referenced from plugin.xml and Kotlin code.
//
// IntelliJ Platform requires icons to be loaded once into a static field so
// they can be cached and serialised across processes (sandbox IDE, plugin
// verifier, etc.).

package dev.mighty.jetbrains

import com.intellij.openapi.util.IconLoader
import javax.swing.Icon

object MightyIcons {
    @JvmField
    val File: Icon = IconLoader.getIcon("/icons/mty_file.svg", MightyIcons::class.java)

    @JvmField
    val ToolWindow: Icon = IconLoader.getIcon("/icons/tool_window.svg", MightyIcons::class.java)
}
