# What arrives out of order in the daemon

The daemon has no user interface and no background work of its own beyond two timers. Everything
else is a request, and requests arrive at any time and in any order. This page lists each
independent arrival, each pair that touches the same state, and what keeps the pair safe. Keep it
current when you change any of these.

## Arrivals

| Arrival | Touches |
| --- | --- |
| An upload (`POST /rpm/nas/publish`) | the upload file of that owner computer, the snapshot, that computer's trust list |
| A picture upload (`PUT /rpm/nas/art/…`) | one picture file |
| A session request (`/rpm/session`) | the challenge table, the token table |
| Reads: catalogue, track, cover, disc, picture (many at once) | the snapshot, files in the library folder, picture files |
| The claim file changing on the NAS (written by an owner computer over SMB) | who counts as an owner |
| Library files changing on the NAS (an import, a tag edit, a move, over SMB) | the files that reads open |
| A library folder appearing or disappearing under `root` (rescan every 10 seconds) | which libraries are served and announced |
| The space measurement (every 10 minutes and after each upload) | the used and free numbers |
| A restart | everything is read again from the data folder |

## Pairs

**Upload and a catalogue read.** The snapshot is one `Arc` behind a lock. A read takes a clone of the
`Arc` and serves from it, and an upload replaces the `Arc`. A read that started before the upload
finishes with the old catalogue, whole. It never mixes the two. *Verified by construction; the lock
is held only to swap or clone the `Arc`.* What makes it safe: the snapshot is replaced, never
changed in place. Changing a field of the current snapshot would break this.

**Upload and a track read.** The same: a track read resolves its path from the snapshot it cloned.
If the new catalogue no longer has the track, the read that already started still serves the file.
*Safe.*

**Upload and the trust check.** The trust lists are read on every request, under the same lock. A
key removed by an upload stops working at the next request, including requests made with a token or
a media ticket issued earlier (`Sessions::reader` asks the trust check each time). A stream that is
already sending bytes is not cut off mid-file. *Verified by
`removed_key_stops_reading_after_the_next_upload` and `token_stops_working_when_key_leaves_trust_list`.*

**Two uploads at once (from two owner computers, or one computer retrying).** Each owner computer
has its own upload file, written to a temporary name and renamed, so the files never mix. The
snapshot is the upload with the later arrival time; two uploads that arrive in the same millisecond
keep the one applied last. Both computers build from the same files, so either is correct. *Safe.*

**Upload cut off, or the daemon stops while saving.** The body is read whole before anything is
saved, and the file is saved before the snapshot changes. A cut-off body is answered 400 and
changes nothing; a stop while saving leaves the old file, because the rename has not happened.
*Verified by `interrupted_publish_keeps_previous_catalogue` and `restart_keeps_device_id_and_catalogue`.*

**Claim file change and an upload.** The claim file is read again when its size or modified time
changes, at the next request. An owner computer added to the file can upload from its next session.
A computer removed from the file can still upload with a session it already has, until the token
runs out (30 minutes): `is_owner` is asked when the upload arrives, so in fact it is refused at once.
*Verified by `claim_change_is_read_without_restart`.* Not verified: two programs writing the claim
file at the same moment over SMB. The owner computers write it to a temporary name and rename it.

**Library files changing and a track read.** A file replaced over SMB while it is being streamed:
the stream keeps the file it opened (on Linux the old data stays readable until it is closed). A
file that is gone when the read starts is answered 404, and the reader tries its next source.
*Assumed safe for Samba on Linux; not verified on other systems.*

**Folder rescan and requests to a library that disappeared.** When a folder disappears, the
library's listener is stopped and its announcement withdrawn. A request already running finishes
with 404s for missing files. *Safe.* A folder that is renamed is a disappearance and an appearance;
the library id file moves with it, so the same key and uploads are used again.

**Space measurement and a hello.** The numbers are atomics; a hello may show the numbers from
before the latest import. *Safe; they are a guide for the owner's computer, which checks before each
import.*

## What to look at next

- Stopping a stream mid-file when its key is removed. Today it finishes the file.
- Two owner computers writing the claim file at the same moment.
