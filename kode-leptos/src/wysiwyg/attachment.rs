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
