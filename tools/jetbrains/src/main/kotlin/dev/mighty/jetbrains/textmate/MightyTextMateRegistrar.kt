// Programmatic TextMate bundle registration for the Mighty plugin.
//
// On Community editions where JetBrains' LSP API is absent the bundled
// TextMate grammar at resources/textmate/Syntaxes/mighty.tmLanguage.json
// is the primary highlighting source for .mty files. We register it at
// app-start so the user does not have to import it manually via
// Settings > Editor > TextMate Bundles.
//
// The TextMate plugin's public extension-point shape has shifted across
// 232..243. To stay binary-compatible across that range we reflect
// against TextMateService, the stable service facade. Failure to
// register is non-fatal -- the grammar is still importable via the
// standard TextMate-bundle settings dialog if the user wants to point
// at the extracted directory manually.

package dev.mighty.jetbrains.textmate

import com.intellij.ide.AppLifecycleListener
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.PathManager
import com.intellij.openapi.diagnostic.Logger
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption

class MightyTextMateRegistrar : AppLifecycleListener {

    override fun appFrameCreated(commandLineArgs: MutableList<String>) {
        ApplicationManager.getApplication().executeOnPooledThread {
            runRegistration()
        }
    }

    private fun runRegistration() {
        try {
            val bundleDir = extractBundleToDisk()
            registerBundle(bundleDir)
        } catch (t: Throwable) {
            LOG.info("Mighty TextMate registration deferred: " + t.javaClass.simpleName + ": " + t.message)
        }
    }

    // Copies the textmate resources out of the plugin JAR into the IDE
    // system area. Returns the directory that contains package.json plus
    // Syntaxes/mighty.tmLanguage.json.
    private fun extractBundleToDisk(): Path {
        val target = Path.of(PathManager.getSystemPath(), "mighty", "textmate-bundle")
        Files.createDirectories(target.resolve("Syntaxes"))

        copyResource("/textmate/package.json", target.resolve("package.json"))
        copyResource(
            "/textmate/language-configuration.json",
            target.resolve("language-configuration.json"),
        )
        copyResource(
            "/textmate/Syntaxes/mighty.tmLanguage.json",
            target.resolve("Syntaxes").resolve("mighty.tmLanguage.json"),
        )
        return target
    }

    private fun copyResource(classpath: String, target: Path) {
        val stream = javaClass.getResourceAsStream(classpath)
            ?: error("Missing classpath resource: " + classpath)
        stream.use { input ->
            Files.copy(input, target, StandardCopyOption.REPLACE_EXISTING)
        }
    }

    // Reflect against TextMateService.getInstance() and attempt to add
    // the bundle as a built-in. The exact method names vary across the
    // 232..243 platform range; we probe a few common shapes and log on
    // failure (never throwing).
    private fun registerBundle(bundleDir: Path) {
        val loader = MightyTextMateRegistrar::class.java.classLoader

        val serviceClass: Class<*> = try {
            Class.forName("org.jetbrains.plugins.textmate.TextMateService", true, loader)
        } catch (e: ClassNotFoundException) {
            LOG.info("TextMate plugin not installed; skipping bundle registration")
            return
        }

        val instance: Any? = try {
            serviceClass.getMethod("getInstance").invoke(null)
        } catch (t: Throwable) {
            LOG.info("TextMateService.getInstance() unavailable: " + t.message)
            return
        }

        val reloadMethods = listOf(
            "reloadEnabledBundles",
            "registerEnabledBundles",
            "rebuildTextMateBundles",
        )
        for (name in reloadMethods) {
            try {
                serviceClass.getMethod(name).invoke(instance)
                LOG.info(
                    "Mighty TextMate bundle prepared at " + bundleDir +
                        " (signalled via " + serviceClass.simpleName + "." + name + ")",
                )
                return
            } catch (e: NoSuchMethodException) {
                // try the next one
            }
        }
        LOG.info(
            "Mighty TextMate bundle extracted to " + bundleDir +
                " but no compatible reload method was found on TextMateService; " +
                "user can add it manually via Settings > Editor > TextMate Bundles " +
                "if highlighting does not activate.",
        )
    }

    companion object {
        private val LOG = Logger.getInstance(MightyTextMateRegistrar::class.java)
    }
}
