// Semantic error types for precise error handling

pub const MarkupError = error{
    UnmatchedTag,
    InvalidColorName,
    InvalidStyleAttribute,
    NestedTagMismatch,
};

pub const RenderError = error{
    OutOfMemory,
    InvalidWidth,
    ContentTooLarge,
};

pub const TableError = error{
    ColumnCountMismatch,
    InvalidSpan,
    OutOfMemory,
};

pub const ConsoleError = error{
    WriteError,
    InvalidTerminal,
    UnsupportedOperation,
};
