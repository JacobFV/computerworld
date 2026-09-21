# Computers, files, processes and shell

Every `ComputerDefinition` constructs an independent machine from an OS profile.
Profiles control synthetic home/path/case/shell behavior. They do not invoke host
macOS, Windows or Linux programs. A multi-machine environment does not share files,
process IDs or working directories unless an explicit service operation transfers
data.

`cw-computer::Vfs` models inode identity, files, directories, links, symlinks and
case-aware path resolution with copy-on-write backing. Trusted internal accessors
are distinct from permission-checked `read_as`/`write_as` operations; actor-facing
kernel access uses the machine's synthetic user. Host paths are never backing
storage for these files.

`ProcessTable` models spawn, exit, sleep/advance, signals, file descriptors and
listener cleanup. Time-dependent changes use logical time
([determinism](determinism.md)); a program running on a machine can be stopped and
stepped from an editor ([debugging](debugging.md)). This is a simulated
process model, not execution of arbitrary binaries. `PackageManager` supports
registered packages, dependency resolution and install transactions.

The shell interprets a supported command subset through `Computer::execute` and
an explicit `ShellHost` capability. [Shell](shell.md) is the command matrix, including
which of `python3` and `node` a machine has and what their consoles do. Network commands call synthetic networking;
no command launches a host subprocess. Unsupported commands should fail visibly.
Do not assume arbitrary POSIX shell or PowerShell script compatibility.

Git behavior models repositories and content-addressed commits with a canonical
SHA-256 JSON protocol for remote transfer. It is suitable for synthetic clone,
commit, push and fetch tasks across world machines. It is not the real Git pack
format, smart HTTP wire protocol or SHA-1 object identity. Adapt workflows and
fixtures rather than treating existing `.git` directories as directly compatible.

Focused implementation and regression tests live in
[`crates/computer`](../crates/computer). The
[company example](../examples/native/company.rs) demonstrates composition with
network services.

A machine's own definition — its OS profile, home, packages and the node it sits on — is
part of the [world schema](world-schema.md); what an actor may do to it is the grant in
the [agent API](agent-api.md), and what it may never reach is [security](security.md).
The [desktop](desktop-gui.md) is what all of this looks like on a screen.
