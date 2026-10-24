pub enum CategoryKind {
    Prefix(&'static str),
    Explicit(&'static [&'static str]),
}

pub struct CategoryDef {
    pub name: &'static str,
    pub kind: CategoryKind,
}

pub const CATEGORIES: &[CategoryDef] = &[
    CategoryDef { name: "Images", kind: CategoryKind::Prefix("image/") },
    CategoryDef { name: "Audio", kind: CategoryKind::Prefix("audio/") },
    CategoryDef { name: "Video", kind: CategoryKind::Prefix("video/") },
    CategoryDef { name: "Text", kind: CategoryKind::Prefix("text/") },
    CategoryDef {
        name: "Documents",
        kind: CategoryKind::Explicit(&[
            "application/pdf",
            "application/rtf",
            "text/rtf",
            "application/epub+zip",
            "text/markdown",
            "application/vnd.oasis.opendocument.text",
            "application/vnd.oasis.opendocument.spreadsheet",
            "application/vnd.oasis.opendocument.presentation",
            "application/msword",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.ms-excel",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "application/vnd.ms-powerpoint",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "application/x-mobipocket-ebook",
            "application/vnd.amazon.ebook",
        ]),
    },
    CategoryDef {
        name: "Archives",
        kind: CategoryKind::Explicit(&[
            "application/zip",
            "application/x-tar",
            "application/gzip",
            "application/x-gzip",
            "application/x-bzip2",
            "application/x-xz",
            "application/zstd",
            "application/x-zstd",
            "application/x-7z-compressed",
            "application/x-rar",
            "application/vnd.rar",
            "application/x-lzip",
            "application/x-lzma",
            "application/x-brotli",
        ]),
    },
];
