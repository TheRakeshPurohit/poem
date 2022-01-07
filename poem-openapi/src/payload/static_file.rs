use poem::{error::StaticFileError, Error, IntoResponse, Response};

use crate::{
    registry::{
        MetaHeader, MetaMediaType, MetaResponse, MetaResponses, MetaSchema, MetaSchemaRef, Registry,
    },
    ApiResponse,
};

/// A response for static files.
#[cfg_attr(docsrs, doc(cfg(feature = "i18n")))]
pub struct StaticFile(Result<Response, StaticFileError>);

impl StaticFile {
    /// Create a static file response.
    pub fn new(res: Result<Response, StaticFileError>) -> Self {
        Self(res)
    }
}

impl ApiResponse for StaticFile {
    fn meta() -> MetaResponses {
        MetaResponses {
            responses: vec![MetaResponse {
                description: "File content",
                status: None,
                content: vec![MetaMediaType {
                    content_type: "application/octet-stream",
                    schema: MetaSchemaRef::Inline(Box::new(MetaSchema {
                        format: Some("binary"),
                        ..MetaSchema::new("string")
                    })),
                }],
                headers: vec![
                    MetaHeader {
                        name: "ETag",
                        description: None,
                        required: false,
                        schema: MetaSchemaRef::Inline(Box::new(MetaSchema::new("string"))),
                    },
                    MetaHeader {
                        name: "Last-Modified",
                        description: None,
                        required: false,
                        schema: MetaSchemaRef::Inline(Box::new(MetaSchema::new_with_format(
                            "string",
                            "date-time",
                        ))),
                    },
                ],
            }],
        }
    }

    fn register(_registry: &mut Registry) {}
}

impl IntoResponse for StaticFile {
    fn into_response(self) -> Response {
        match self.0 {
            Ok(resp) => resp,
            Err(err) => Error::from(err).as_response(),
        }
    }
}
