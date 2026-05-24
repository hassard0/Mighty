# 02 missing_handler

Protocol Echo declares Ping + Bye; agent Echoer implements only
`on Ping`. The unimplemented `Bye` message must produce SD4032.
Spec §13.
