# NTFS test volume

`testfs1.gz` is a 2 MiB NTFS volume (gzip-compressed) made with `mkntfs` and filled by
`create-testfs1.sh`: a few small files (resident, non-resident, sparse) and a folder with 512
subfolders, so the file table needs index blocks.

Both files come from the [`ntfs`](https://github.com/ColinFinck/ntfs) crate's test data,
Copyright 2021 Colin Finck, used under the MIT licence (`LICENSE-MIT`; the crate is MIT OR Apache-2.0).
