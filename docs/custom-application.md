# Create an application

Start from [`examples/custom-app`](../examples/custom-app). Implement `Application`
and register it independently of the kernel. Put persistent semantic state in its
instance value; derive a native `Page` from immutable state.

Define event names and stable target IDs. Return explicit effects for filesystem
or network work, and update the visible page from completed results. Test state
transitions separately from layout, then test hit testing and actor interaction.
The same app can be observed structurally without rasterization or drawn to RGBA
for a vision agent.

Keep OS-specific mechanics in the computer substrate and transport mechanics in
networking. The application should not directly inspect other machines or service
stores. See [application-sdk.md](application-sdk.md).
