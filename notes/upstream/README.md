# Upstream

Bugs and gaps found in dependencies while building this app, one file per crate: what was
hit, how it was told apart from our own mistake, what it cost here, and whether it has been
reported. A bug gets a note when it's found, whether or not it's worked around. A workaround
isn't required, but every workaround must be in its bug's note, with where it lives, so that
one whose reason has gone can be taken out. Notes are rewritten when the dependency moves.

Each file may end with a **`## Wanted`** section: not bugs, but features the app would use if
the crate offered them, each with what the app does instead and where. The point is the same
as the bugs': a release that adds one should be noticed, and the substitute taken out.
Something wanted becomes a bug the moment the crate claims to do it and does not.
