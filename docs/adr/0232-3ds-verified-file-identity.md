# ADR 0232 — Verified 3DS file identity

Status: Accepted. Launching a 4 GiB 3DS game took about 10 seconds, almost all
of it re-hashing the cartridge.

Launch still verifies every 3DS file against its imported SHA-256 before the
game runs, but the hash runs while Azahar loads, and only SHA-256 is computed.
After a successful hash, the held file's identity is recorded in the game's
folder: device, inode, size, and modification and change times to the
nanosecond. A later launch whose held file has exactly that identity is not
read again. Rewriting the file in place or replacing it changes its change
time, which ordinary tools cannot set; any difference, or a missing or
unreadable record, hashes again. The record is advisory data owned by Horizon,
never a source of identity.

Other systems keep hashing on every launch: their files are small enough that
the overlapped hash costs nothing visible.
