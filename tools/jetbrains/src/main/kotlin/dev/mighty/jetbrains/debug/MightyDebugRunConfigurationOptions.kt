// v0.32 Track A — persisted state for the Mighty debug configuration.
//
// Subclasses IntelliJ's RunConfigurationOptions and uses the standard
// delegated-property pattern: each `string(...).provideDelegate(...)`
// call hooks the field into the IDE's XML serialiser so it round-trips
// through `.idea/workspace.xml` automatically. Pattern is taken straight
// from the IntelliJ Platform docs ("Custom Run Configurations" → "State
// Persistence").

package dev.mighty.jetbrains.debug

import com.intellij.execution.configurations.RunConfigurationOptions

open class MightyDebugRunConfigurationOptions : RunConfigurationOptions() {
    /** Absolute path to the .mty file to debug. */
    private val programOption = string("").provideDelegate(this, "program")
    var program: String
        get() = programOption.getValue(this).orEmpty()
        set(v) { programOption.setValue(this, v) }

    /** Optional path to a recorded trace to replay. */
    private val replayTraceOption = string("").provideDelegate(this, "replayTrace")
    var replayTrace: String
        get() = replayTraceOption.getValue(this).orEmpty()
        set(v) { replayTraceOption.setValue(this, v) }

    /** Optional path to write a fresh recorded trace to. */
    private val recordTraceOption = string("").provideDelegate(this, "recordTrace")
    var recordTrace: String
        get() = recordTraceOption.getValue(this).orEmpty()
        set(v) { recordTraceOption.setValue(this, v) }

    /** Argv forwarded to `std.env.args()` (one per line). */
    private val argsOption = string("").provideDelegate(this, "programArgs")
    var programArgs: String
        get() = argsOption.getValue(this).orEmpty()
        set(v) { argsOption.setValue(this, v) }

    /** Pause at the entry of `main`. */
    private val stopOnEntryOption = property(false).provideDelegate(this, "stopOnEntry")
    var stopOnEntry: Boolean
        get() = stopOnEntryOption.getValue(this)
        set(v) { stopOnEntryOption.setValue(this, v) }
}
