# The protocol

The daemon speaks the rpm peer protocol, the same one an rpm computer uses to share its collection.
The protocol is defined by `RPM.Core/Sharing` in the
[rpm-desktop](https://github.com/rpmrpmrpm/rpm-desktop) repo. This page describes the part the
daemon implements, and the routes it adds. Where this page and `RPM.Core` disagree, `RPM.Core` is
right and this page and the daemon are wrong.

`crates/rpm-peer/tests/vectors/identity.json` holds values made with openssl and Python: a key, its
device id, a signature, a token, its media handle and ticket. Both `RPM.Core` and the daemon test
against the same file.

## Announcement

Multicast DNS, service type `_rpm-nas._tcp`. rpm computers announce `_rpm._tcp`; the NAS uses its own
type so an rpm build that does not know about a NAS never sees one.

The daemon announces the NAS once, and each library folder once.

| TXT key | Value |
| --- | --- |
| `v` | Protocol version, `4` |
| `id` | Device id |
| `n` | Name to show: the NAS name, or the library's name |
| `k` | Public key, base64url |
| `r` | `nas` or `library` |
| `l` | Library id (libraries only) |
| `s` | SMB address of the folder that holds the libraries (the NAS only) |

A reader drops a record whose `id` is not the id its `k` gives.

## Keys and ids

- The device key is ECDSA P-256. On the wire, a public key is its SubjectPublicKeyInfo DER, in
  base64url without padding.
- The device id is Crockford base32 (`0123456789ABCDEFGHJKMNPQRSTVWXYZ`, most significant bit first,
  no padding) of the first 16 bytes of the SHA-256 of the public key.
- A signature is ECDSA with SHA-256, written as 64 bytes: r, then s.

## TLS

HTTPS, HTTP/1.1, one request per connection. The certificate is self-signed over the device key,
with subject `CN=<device id>`. Readers do not check a chain or a date. They check that the public key
in the certificate is the key they already hold for that device, and close the connection otherwise.

Limits: 16 KB for the request line and headers, 15 seconds for the handshake and the headers, 64
connections at once. Methods other than GET, HEAD, POST and PUT are answered 405.

## Routes anyone may use

**`GET /rpm/hello`** answers JSON:

| Field | Meaning |
| --- | --- |
| `version` | `4` |
| `deviceId`, `name`, `publicKey` | As in the announcement |
| `trackCount` | Tracks in the catalogue (0 for the NAS) |
| `libraryId` | The library id. Libraries only |
| `nas` | `smb`, `limitBytes` (left out when there is no limit), `usedBytes`, `freeBytes`, and for the NAS `libraries`: each `{ folder, libraryId, name, account, deviceId, publicKey }`, with an empty `folder` for a library that is the folder `smb` names itself, and `existingMusic`: `true` when that folder holds music files (at most four folders down, outside hidden and system folders) and no library, so a computer may make the folder itself a library |

**`GET /rpm/session`** answers `{ "challenge", "expiresIn" }`. The challenge is 24 random bytes in
base64url, and can be answered once, within 60 seconds. At most 256 are open; a new one beyond that
drops the oldest.

**`POST /rpm/session`** takes `{ "publicKey", "challenge", "signature", "account" }`. The signature is
over the UTF-8 bytes of:

```
rpm-share-session
<device id of the daemon peer being asked>
<challenge>
<account, or empty>
```

with a newline between the lines and none at the end. For a key the trust check accepts, the answer is
`{ "token", "expiresIn": 1800, "kind" }`, where `kind` is a number: 0 for the owner's own devices, 1
for a guest. Otherwise the answer is 403. The challenge is used up either way.

## Routes that need a token

The token goes in `Authorization: Bearer <token>`. The trust check is asked again on every request,
so a key that the owner's computer stops trusting stops working at once. Without a token the answer
is 401.

The media routes (`/rpm/track/`, `/rpm/cover/`, `/rpm/disc/`, `/rpm/art/`) also accept a ticket in
the URL instead, because a media player cannot set a header: `?s=<handle>&t=<ticket>`.

- handle = base64url of the first 16 bytes of SHA-256 over `rpm-media-handle\n<token>`
- ticket = base64url of the first 16 bytes of HMAC-SHA256 with the token as the key, over
  `rpm-media\n<route><id>`, where route is for example `/rpm/track/` and id is the decoded id

A ticket opens its own route and id only.

| Route | Answer |
| --- | --- |
| `GET /rpm/catalog?have=<version>` | The catalogue the owner's computer uploaded: the owner variant for kind 0, the guest variant for kind 1. 204 when `have` is its version. 503 before the first upload |
| `GET /rpm/track/<rpm id>` | The file, with `Accept-Ranges: bytes`. `Range` is one range only (`bytes=a-b`, `bytes=a-`, `bytes=-n`); anything else gets the whole file. 404 for a track the owner keeps only in the cloud, a missing file, or a path that leaves the library folder |
| `GET /rpm/cover/<rpm id>` | The album's cover: the folder's image (`cover`, `folder`, `front`, `album`, `albumart` with `.jpg`, `.jpeg`, `.png`, `.webp`, in the order `RPM.Core` uses, matched without regard to case), else the only picture in the folder, else the picture in the file, else the picture in another track of the same folder. 404 when there is none |
| `GET /rpm/disc/<rpm id>` | `disc.png`, `disc.jpg` or `disc.jpeg` in the album's folder |
| `GET /rpm/art/<key>` | A picture the owner's computer uploaded, such as `pl:12` for a playlist |

Pictures are sent with `Cache-Control: public, max-age=31536000, immutable`.

## Routes for the owner's computer

An owner is a key listed in the library's claim file (below). Other readers get 403.

**`POST /rpm/nas/publish`** takes, at most 64 MB:

```json
{ "owner": <PeerCatalog>, "guest": <PeerCatalog>, "trust": [ { "publicKey": "...", "kind": 0 } ] }
```

The catalogues are stored and sent on byte for byte; the daemon reads only `version` and, for each
track, `rpmId`, `path` and `cloudOnly`. The trust list replaces the one this owner computer sent
before. The answer is 204, 400 for a body that is not a valid upload (nothing changes), or 413.

When several owner computers upload, the daemon serves the catalogue that arrived last and accepts a
reader that is in any of their trust lists.

**`PUT /rpm/nas/art/<key>`** takes a picture of at most 4 MB. The key may hold letters, digits, `:`,
`-` and `_`, up to 64 characters. The answer is 204.

## Files in a library folder

| File | Written by | Meaning |
| --- | --- | --- |
| `.rpm/library.json` | rpm | `{"id":"<32 lowercase hex>"}`. A folder without it is not a library. When the daemon's own folder holds it, that folder is the only library, and its subfolders are the library's folders |
| `.rpm/daemon-owner.json` | rpm | `{ "keys": [ { "publicKey", "name", "account" } ] }`: the owner computers. `account` is base64url of the first 12 bytes of SHA-256 of the account's user id, or empty |

The daemon never writes inside the library folders. It keeps its keys and the uploads in its own
data folder.
