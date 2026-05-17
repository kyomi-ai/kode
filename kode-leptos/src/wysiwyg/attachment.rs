//! Types for image/file attachment interaction callbacks.

/// Identifies the type of attachment node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentNodeType {
    Image,
    File,
}

/// Request to delete an attachment (user clicked the X button).
#[derive(Clone, Debug)]
pub struct DeleteAttachmentRequest {
    pub attachment_id: Option<String>,
    pub src_or_href: String,
}

/// Request triggered when user clicks an image or file chip.
#[derive(Clone, Debug)]
pub struct ClickAttachmentRequest {
    pub attachment_id: Option<String>,
    pub src_or_href: String,
    pub node_type: AttachmentNodeType,
}

/// What the host app returns after a successful upload.
#[derive(Clone, Debug)]
pub enum AttachmentInsert {
    Image {
        src: String,
        alt: String,
        attachment_id: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    },
    File {
        href: String,
        filename: String,
        attachment_id: Option<String>,
        size_bytes: Option<u64>,
        content_type: Option<String>,
    },
}

/// Information passed to the on_upload callback.
/// The host app uses this to start the upload and call back with the result.
#[derive(Clone, Debug)]
pub struct UploadTrigger {
    /// Original filename.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// MIME type (e.g., "image/png").
    pub content_type: String,
    /// Unique ID for this upload instance (matches the placeholder).
    pub placeholder_id: String,
}

/// Signal value for completing an upload.
#[derive(Clone, Debug)]
pub struct UploadComplete {
    /// The placeholder_id that was given in the UploadTrigger.
    pub placeholder_id: String,
    /// The attachment to insert. None = upload failed, remove placeholder.
    pub insert: Option<AttachmentInsert>,
}
