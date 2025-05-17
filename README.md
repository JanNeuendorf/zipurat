# zipurat

This repo contains a description of the `.zprt` archive format and a cli tool
for interacting with it.

## Why does this exist?

We all have that archive from when we stopped using a cloud service or made a
last backup from an old laptop. The files are there, probably more than once,
but how do we use them? Usually the only way to get to a file is to copy the
entire archive from wherever it is stored, decrypt and decompress it in its
entirety, and then search for what we want.

Some backup tools fare a lot better in this regard, but they have their own
shortcomings:

- They are usually a lot more complex because they are designed to facilitate
  the entire backup process. That means that their underlying storage format can
  be very complicated.
- They are often not very _programmer friendly_. Ideally, we want to be able to
  easily access old files in scripts.

## The goals

- Very fast indexing and single file access
- Optimized for access over sftp
- Sensible encryption (Some information can leak, but not the contents of
  files.)
- Simple and well described format (It should be possible to get your data
  without this repo.)
- Small files (thanks to deduplication and compression)

## The non-goals

This is not a solution for creating backups, but for when you already have
backups and want to organize them differently. This is not meant to deal with
datasets that are still evolving. Therefore, creating the archive is allowed to
be slow and inconvenient because you will only do it once.

There is no support for anything but file contents: no metadata, no links. The
only exception are empty directories.

There is no error correction used inside the format.

## The design

### Existing technologies

There is no need to reinvent any of the underlying tecnologies or formats. One
just has to make a reasonable choice. Here are the choices for zipurat:

- age for encryption
- zstd for compression
- blake3 for hashes

### The format

zipurat uses its own binary format. It is, however really easy to understand.

It is detailed in ...todo!

## The name

zipurat=zip+ziggurat (a once impressive building that is now a pile of stone to
rummage through).
