use percent_encoding::percent_decode_str;
use uni_core::model::FileEntry;

/// The entries of a PROPFIND response, with paths relative to `root_path` and
/// without the listed directory itself.
pub fn parse(xml: &str, root_path: &str, listed: &str) -> Result<Vec<FileEntry>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();

    for response in doc
        .descendants()
        .filter(|n| n.has_tag_name(("DAV:", "response")))
    {
        let text = |name: &str| {
            response
                .descendants()
                .find(|n| n.has_tag_name(("DAV:", name)))
                .and_then(|n| n.text())
                .map(str::trim)
        };
        let Some(href) = text("href") else { continue };
        let decoded = percent_decode_str(href).decode_utf8_lossy();
        // Some servers answer with an absolute URL, others with a path.
        let href_path = decoded
            .find(root_path)
            .map_or(&*decoded, |at| &decoded[at..]);
        let Some(relative) = href_path.strip_prefix(root_path) else {
            continue;
        };
        let path = relative.trim_matches('/').to_owned();
        if path == listed {
            continue;
        }

        entries.push(FileEntry {
            path,
            is_dir: response
                .descendants()
                .any(|n| n.has_tag_name(("DAV:", "collection"))),
            size: text("getcontentlength").and_then(|s| s.parse().ok()),
            modified: text("getlastmodified").map(str::to_owned),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
 <d:response><d:href>/remote.php/dav/files/ab123456/Uni/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/ab123456/Uni/Analysis%20I/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/ab123456/Uni/notes.md</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>42</d:getcontentlength>
   <d:getlastmodified>Fri, 09 Oct 2026 10:00:00 GMT</d:getlastmodified></d:prop></d:propstat></d:response>
</d:multistatus>"#;

    #[test]
    fn lists_children_with_relative_paths() {
        let entries = parse(SAMPLE, "/remote.php/dav/files/ab123456/", "Uni").expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "Uni/Analysis I");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].path, "Uni/notes.md");
        assert_eq!(entries[1].size, Some(42));
        assert!(!entries[1].is_dir);
    }
}
