## Structure

The file consists of the following blocks:

- A magic number
- The files
- The Index
- The Index length
- The magic number repeated

### The magic number

The file begins and ends with a unsigned 64 bit integer encoded in le bytes. It
serves two purposes:

- Signify that this is a .zprt file and that it has been completely written or
  copied.
- Serve as a version signifier if there are breaking changes to the format.

For version 1.0 the number is 12219678139600706333.

### The files

The files are written in arbitrary order. Ideally, the order is randomized to
obfuscate patterns in the file sizes.

Each file is compressed using zstd and then encrypted with age. The results are
simply written to the archive in sequence.

Doing this means that each file carries its own age header. There are two
reasons this is done:

- In case of corruption of the index it should at least be possible to tell
  where files begin and end, and to decrypt them manually.

- It is possible to use different recipients for different files. Someone get
  access to the entire index and certain files but not to the content of
  restricted files.

  ### The index

  The index itself is also zstd compressed and then age encrypted. It uses its
  own binary serialization.

  The serialization follows the following rules:

  - All numbers mentioned are unsigned 64 bit integers and get encoded to le
    bytes.
  - A hash is a blake3 hash and its 32 bytes are just written as they are.
  - The combination `(index,len)` is encoded as two numbers in a row.
  - A list is encoded as its length followed by all its elements serialized in
    sequence.
  - Strings are encoded by their length followed by their utf8 encoded content.

  Paths are always relative. They are encoded as a list of their components.
  These components are represented as strings. There is no rule for what to do
  with non-utf8 paths. They could be ignored or renamed, but they can not be
  represented in the format.

  The index is written as:

  - The magic number (repeated again for convenience and to avoid tampering.)
  - A list of (index,len) that correspond to the start-bytes and lengths of all
    unique files. The lengths are the lengths of the encrypted and compressed
    files.
  - A list of hashes corresponding to the unique files from the previous list.
    These are the hashes of the original files.
  - A list of sizes (in bytes) of the original files. This list matches the
    order of the previous two.
  - A second list of (index, len), this time including duplicates for duplicate
    files. (It is a superset of the first list.)
  - A list of paths in the order of the previous list, giving the mapping of
    which path is stored at which index.
  - A list of paths that are empty directories

### Finding the index

Next, we store the length of the compressed and encrypted index. This
information is always at a fixed position (starting 16 bytes from the end). When
using sftp, the index in the file is usually tracked locally and used to make
read-calls at absolute positions, so we might as well store the absolute
position of the index in the file. But maybe this is used in some scenario where
seeking to an absolute position is costly.
