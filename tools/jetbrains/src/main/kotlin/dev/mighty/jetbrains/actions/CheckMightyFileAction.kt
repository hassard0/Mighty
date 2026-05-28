// Type-check the active .mty file via `mty check`.

package dev.mighty.jetbrains.actions

class CheckMightyFileAction : MightyTerminalAction(
    subcommand = "check",
    tabTitle = "mty check",
)
