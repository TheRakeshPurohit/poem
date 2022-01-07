use std::{
    fs::Metadata,
    path::Path,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use headers::{ETag, HeaderMapExt, IfMatch, IfModifiedSince, IfNoneMatch, IfUnmodifiedSince};
use http::{header, StatusCode};
use httpdate::HttpDate;
use mime::Mime;
use tokio::fs::File;

use crate::{error::StaticFileError, Body, FromRequest, Request, RequestBody, Response, Result};

/// An extractor for responding static files.
pub struct StaticFile {
    if_match: Option<IfMatch>,
    if_unmodified_since: Option<IfUnmodifiedSince>,
    if_none_match: Option<IfNoneMatch>,
    if_modified_since: Option<IfModifiedSince>,
}

#[async_trait::async_trait]
impl<'a> FromRequest<'a> for StaticFile {
    async fn from_request(req: &'a Request, _body: &mut RequestBody) -> Result<Self> {
        Ok(Self {
            if_match: req.headers().typed_get::<IfMatch>(),
            if_unmodified_since: req.headers().typed_get::<IfUnmodifiedSince>(),
            if_none_match: req.headers().typed_get::<IfNoneMatch>(),
            if_modified_since: req.headers().typed_get::<IfModifiedSince>(),
        })
    }
}

impl StaticFile {
    /// Create static file response.
    ///
    /// `prefer_utf8` - Specifies whether text responses should signal a UTF-8
    /// encoding.
    pub fn create_response(
        self,
        path: impl AsRef<Path>,
        prefer_utf8: bool,
    ) -> Result<Response, StaticFileError> {
        let path = path.as_ref();
        let guess = mime_guess::from_path(path);
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        let mut builder = Response::builder();

        // content type
        if let Some(mut mime) = guess.first() {
            if prefer_utf8 {
                mime = equiv_utf8_text(mime);
            }
            builder = builder.header(header::CONTENT_TYPE, mime.to_string());
        }

        if let Ok(modified) = metadata.modified() {
            let etag = etag(ino(&metadata), &modified, metadata.len());

            if let Some(if_match) = self.if_match {
                if !if_match.precondition_passes(&etag) {
                    return Ok(builder.status(StatusCode::PRECONDITION_FAILED).finish());
                }
            }

            if let Some(if_unmodified_since) = self.if_unmodified_since {
                if !if_unmodified_since.precondition_passes(modified) {
                    return Ok(builder.status(StatusCode::PRECONDITION_FAILED).finish());
                }
            }

            if let Some(if_non_match) = self.if_none_match {
                if !if_non_match.precondition_passes(&etag) {
                    return Ok(builder.status(StatusCode::NOT_MODIFIED).finish());
                }
            } else if let Some(if_modified_since) = self.if_modified_since {
                if !if_modified_since.is_modified(modified) {
                    return Ok(builder.status(StatusCode::NOT_MODIFIED).finish());
                }
            }

            builder = builder
                .header(header::CACHE_CONTROL, "public")
                .header(header::LAST_MODIFIED, HttpDate::from(modified).to_string());
            builder = builder.typed_header(etag);
        }

        Ok(builder.body(Body::from_async_read(File::from_std(file))))
    }
}

fn equiv_utf8_text(ct: Mime) -> Mime {
    if ct == mime::APPLICATION_JAVASCRIPT {
        return mime::APPLICATION_JAVASCRIPT_UTF_8;
    }

    if ct == mime::TEXT_HTML {
        return mime::TEXT_HTML_UTF_8;
    }

    if ct == mime::TEXT_CSS {
        return mime::TEXT_CSS_UTF_8;
    }

    if ct == mime::TEXT_PLAIN {
        return mime::TEXT_PLAIN_UTF_8;
    }

    if ct == mime::TEXT_CSV {
        return mime::TEXT_CSV_UTF_8;
    }

    if ct == mime::TEXT_TAB_SEPARATED_VALUES {
        return mime::TEXT_TAB_SEPARATED_VALUES_UTF_8;
    }

    ct
}

#[allow(unused_variables)]
fn ino(md: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::ino(md)
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn etag(ino: u64, modified: &SystemTime, len: u64) -> ETag {
    let dur = modified
        .duration_since(UNIX_EPOCH)
        .expect("modification time must be after epoch");

    ETag::from_str(&format!(
        "\"{:x}:{:x}:{:x}:{:x}\"",
        ino,
        len,
        dur.as_secs(),
        dur.subsec_nanos()
    ))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use super::*;

    #[test]
    fn test_equiv_utf8_text() {
        assert_eq!(
            equiv_utf8_text(mime::APPLICATION_JAVASCRIPT),
            mime::APPLICATION_JAVASCRIPT_UTF_8
        );
        assert_eq!(equiv_utf8_text(mime::TEXT_HTML), mime::TEXT_HTML_UTF_8);
        assert_eq!(equiv_utf8_text(mime::TEXT_CSS), mime::TEXT_CSS_UTF_8);
        assert_eq!(equiv_utf8_text(mime::TEXT_PLAIN), mime::TEXT_PLAIN_UTF_8);
        assert_eq!(equiv_utf8_text(mime::TEXT_CSV), mime::TEXT_CSV_UTF_8);
        assert_eq!(
            equiv_utf8_text(mime::TEXT_TAB_SEPARATED_VALUES),
            mime::TEXT_TAB_SEPARATED_VALUES_UTF_8
        );

        assert_eq!(equiv_utf8_text(mime::TEXT_XML), mime::TEXT_XML);
        assert_eq!(equiv_utf8_text(mime::IMAGE_PNG), mime::IMAGE_PNG);
    }

    async fn check_response(req: Request) -> Response {
        let static_file = StaticFile::from_request_without_body(&req).await.unwrap();
        static_file
            .create_response(Path::new("Cargo.toml"), false)
            .unwrap()
    }

    #[tokio::test]
    async fn test_if_none_match() {
        let resp = check_response(Request::default()).await;
        assert!(resp.is_ok());
        let etag = resp.header("etag").unwrap();

        let resp = check_response(Request::builder().header("if-none-match", etag).finish()).await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);

        let resp = check_response(Request::builder().header("if-none-match", "abc").finish()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn test_if_modified_since() {
        let resp = check_response(Request::default()).await;
        assert!(resp.is_ok());
        let modified = resp.header("last-modified").unwrap();

        let resp = check_response(
            Request::builder()
                .header("if-modified-since", modified)
                .finish(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);

        let mut t: SystemTime = HttpDate::from_str(modified).unwrap().into();
        t -= Duration::from_secs(1);

        let resp = check_response(
            Request::builder()
                .header("if-modified-since", HttpDate::from(t).to_string())
                .finish(),
        )
        .await;
        assert!(resp.is_ok());

        let mut t: SystemTime = HttpDate::from_str(modified).unwrap().into();
        t += Duration::from_secs(1);

        let resp = check_response(
            Request::builder()
                .header("if-modified-since", HttpDate::from(t).to_string())
                .finish(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_if_match() {
        let resp = check_response(Request::default()).await;
        assert!(resp.is_ok());
        let etag = resp.header("etag").unwrap();

        let resp = check_response(Request::builder().header("if-match", etag).finish()).await;
        assert!(resp.is_ok());

        let resp = check_response(Request::builder().header("if-match", "abc").finish()).await;
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn test_if_unmodified_since() {
        let resp = check_response(Request::default()).await;
        assert!(resp.is_ok());
        let modified = resp.header("last-modified").unwrap();

        let resp = check_response(
            Request::builder()
                .header("if-unmodified-since", modified)
                .finish(),
        )
        .await;
        assert!(resp.is_ok());

        let mut t: SystemTime = HttpDate::from_str(modified).unwrap().into();
        t += Duration::from_secs(1);
        let resp = check_response(
            Request::builder()
                .header("if-unmodified-since", HttpDate::from(t).to_string())
                .finish(),
        )
        .await;
        assert!(resp.is_ok());

        let mut t: SystemTime = HttpDate::from_str(modified).unwrap().into();
        t -= Duration::from_secs(1);
        let resp = check_response(
            Request::builder()
                .header("if-unmodified-since", HttpDate::from(t).to_string())
                .finish(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);
    }
}
