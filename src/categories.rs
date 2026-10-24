pub enum CategoryKind {
    Prefix(&'static str),
    Explicit(&'static [&'static str]),
}

pub struct CategoryDef {
    pub name: &'static str,
    /// Icon name resolved from the hicolor theme (see `icons::icon_uri`).
    pub icon: &'static str,
    pub kind: CategoryKind,
    /// Extra MIME types appended to the category (e.g. application/x-desktop
    /// under Text: a .desktop file is a text file you edit).
    pub extras: &'static [&'static str],
}

pub const CATEGORIES: &[CategoryDef] = &[
    CategoryDef {
        name: "Images",
        icon: "image-x-generic",
        kind: CategoryKind::Prefix("image/"),
        extras: &[],
    },
    CategoryDef {
        name: "Audio",
        icon: "audio-x-generic",
        kind: CategoryKind::Prefix("audio/"),
        extras: &[],
    },
    CategoryDef {
        name: "Video",
        icon: "video-x-generic",
        kind: CategoryKind::Prefix("video/"),
        extras: &[],
    },
    CategoryDef {
        name: "Text",
        icon: "text-x-generic",
        kind: CategoryKind::Prefix("text/"),
        extras: &[
            "application/x-desktop",
            "application/json",
            "application/xml",
            "application/x-yaml",
            "application/toml",
            "application/javascript",
        ],
    },
    CategoryDef {
        name: "Documents",
        icon: "x-office-document",
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
        extras: &[],
    },
    CategoryDef {
        name: "Archives",
        icon: "package-x-generic",
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
        extras: &[],
    },
];
