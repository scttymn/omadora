//! The Browse page: new music, artists on the rise, top artists, moods and
//! activities, and top stations, from the directory Pandora's web player
//! reads. One call answers the page, a section's "view all", or a mood.

use crate::pandora::Pandora;
use anyhow::{Result, anyhow};
use serde_json::{Map, Value, json};

const ART_HOST: &str = "https://content-images.p-cdn.com/";

/// Pictures come from Pandora's own image hosts, or not at all.
fn picture(image: &Value) -> String {
    if let Some(id) = image["artId"].as_str().filter(|id| !id.is_empty() && !id.contains("..")) {
        // A file is served as it is. A playlist's is a collage recipe, or a
        // folder of sizes, that only Pandora's image service renders.
        let file = id.rsplit('/').next().is_some_and(|name| name.contains('.')) && !id.contains(';');
        if !file {
            return crate::catalog::thor(&json!(id.trim_start_matches('/')), 500);
        }
        return format!("{ART_HOST}{}", id.trim_start_matches('/'));
    }
    let full = image["fullUrl"].as_str().unwrap_or_default();
    let host = full.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or_default();
    if host == "www.pandora.com" || host.ends_with(".p-cdn.com") {
        full.to_string()
    } else {
        String::new()
    }
}

fn kind(id: &str) -> Option<&'static str> {
    match id.split(':').next()? {
        "AL" => Some("album"),
        "AR" => Some("artist"),
        "GE" => Some("station"),
        "BR" => Some("category"),
        "PL" => Some("playlist"),
        _ => None,
    }
}

fn labels(item: &Value, style: &str) -> Vec<String> {
    item["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["style"] == style)
        .filter_map(|l| l["text"].as_str().map(str::to_string))
        .collect()
}

/// One tile: what it is, its name, a line under it, and its picture.
fn tile(item: &Value, annotations: &Map<String, Value>) -> Option<Value> {
    let id = item["pandoraId"].as_str()?;
    let kind = kind(id)?;
    let title = labels(item, "title").into_iter().next()?;
    let detail = labels(item, "detail").into_iter().next().unwrap_or_default();
    let color = item["image"]["dominantColor"].as_str().filter(|c| !c.is_empty()).map(|c| format!("#{c}")).unwrap_or_default();
    let about = annotations.get(id).cloned().unwrap_or(Value::Null);
    Some(json!({
        "id": id,
        "kind": kind,
        "title": title,
        "detail": detail,
        "art": if kind == "playlist" { crate::catalog::steady_art(&json!(id), picture(&item["image"])) } else { picture(&item["image"]) },
        "color": color,
        "artist": about["artistName"].as_str().unwrap_or_default(),
        "artistId": about["artistId"].as_str().unwrap_or_default(),
    }))
}

/// The Browse page (nothing given), a section's full list (its `path`, such
/// as /browse/music/new-music), or a mood's stations (its `BR:` id).
pub fn page(pandora: &Pandora, path: Option<&str>, id: Option<&str>) -> Result<Value> {
    let mut body = json!({ "feedType": "browse", "offset": 0 });
    match (path.filter(|p| !p.is_empty()), id.filter(|i| !i.is_empty())) {
        (_, Some(id)) if id.starts_with("BR:") => body["directoryId"] = json!(id),
        (Some(path), _) if path.starts_with("/browse") && !path.contains("..") => body["path"] = json!(path),
        (None, None) => {}
        _ => return Err(anyhow!("There is nothing to browse there.")),
    }
    let found = pandora.call("v1/feed/getDirectory", body)?;
    if found.is_null() {
        return Err(anyhow!("There is nothing to browse there."));
    }
    let annotations = found["annotations"].as_object().cloned().unwrap_or_default();
    let sections: Vec<Value> = found["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let items: Vec<Value> = s["subsections"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|sub| sub["items"].as_array().cloned().unwrap_or_default())
                .filter_map(|item| tile(&item, &annotations))
                .collect();
            if items.is_empty() {
                return None;
            }
            let more = s["viewAll"]["webUrl"].as_str().filter(|p| p.starts_with("/browse")).unwrap_or_default();
            Some(json!({
                "title": labels(&s["title"], "header").into_iter().next().unwrap_or_default(),
                "more": more,
                "items": items,
            }))
        })
        .collect();
    Ok(json!({ "sections": sections }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_only_from_pandora() {
        assert_eq!(picture(&json!({ "artId": "images/ab/500W_500H.jpg" })), "https://content-images.p-cdn.com/images/ab/500W_500H.jpg");
        assert_eq!(picture(&json!({ "fullUrl": "https://www.pandora.com/img/browse_activities/BA44.jpg" })), "https://www.pandora.com/img/browse_activities/BA44.jpg");
        assert_eq!(picture(&json!({ "fullUrl": "https://evil.example/x.png" })), "");
        assert_eq!(picture(&json!({ "fullUrl": "http://www.pandora.com/x.png" })), "");
        assert_eq!(picture(&json!({ "artId": "images/44/f1/66/7e7a/" })), "https://dyn-images.p-cdn.com/?l=images%2F44%2Ff1%2F66%2F7e7a%2F&w=500&h=500");
        assert!(picture(&json!({ "artId": "images/thor_assets/x/theme_TH-4-1;grid(images/b/@1/3,images/a/@1/3);overlay($0)" }))
            .starts_with("https://dyn-images.p-cdn.com/?l=images%2Fthor_assets%2Fx%2Ftheme_TH-4-1%3Bgrid%28images%2Fa%2F%401%2F3%2Cimages%2Fb%2F"));
    }

    #[test]
    fn tiles_say_what_they_are() {
        let item = json!({
            "pandoraId": "AL:1",
            "labels": [{ "style": "title", "text": "An Album" }, { "style": "detail", "text": "Someone" }],
            "image": { "artId": "images/a.jpg", "dominantColor": "4f5534" }
        });
        let mut annotations = Map::new();
        annotations.insert("AL:1".into(), json!({ "artistName": "Someone", "artistId": "AR:2" }));
        let t = tile(&item, &annotations).unwrap();
        assert_eq!(t["kind"], "album");
        assert_eq!(t["detail"], "Someone");
        assert_eq!(t["artistId"], "AR:2");
        assert_eq!(t["color"], "#4f5534");
        assert!(tile(&json!({ "pandoraId": "PC:1", "labels": [{ "style": "title", "text": "A podcast" }] }), &annotations).is_none());
    }
}
