use super::{Episode, Feed, ParsedFeed};
use reqwest::Url;

pub fn url(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "Enter a valid RSS URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("RSS URLs must use HTTP or HTTPS".into());
    }
    url.set_fragment(None);
    Ok(url)
}

/// Render feed HTML as inert text; retain paragraph boundaries and decode entities.
pub fn plain(value: &str) -> String {
    let mut output = String::new();
    let mut tag = String::new();
    let mut inside = false;
    let mut suppressed = false;
    for c in value.chars().take(200_000) {
        match c {
            '<' => {
                inside = true;
                tag.clear();
            }
            '>' if inside => {
                inside = false;
                let name = tag
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style") {
                    suppressed = true;
                }
                if matches!(name.as_str(), "/script" | "/style") {
                    suppressed = false;
                }
                if matches!(name.as_str(), "br" | "br/" | "/p" | "/div" | "/li") {
                    output.push('\n');
                }
            }
            _ if inside => tag.push(c),
            _ if !suppressed => output.push(c),
            _ => {}
        }
    }
    html_escape::decode_html_entities(&output).trim().to_owned()
}

pub fn duration(value: &str) -> f64 {
    let parts: Option<Vec<f64>> = value
        .trim()
        .split(':')
        .map(|v| v.parse::<f64>().ok())
        .collect();
    parts
        .filter(|p| !p.is_empty() && p.len() <= 3 && p.iter().all(|n| n.is_finite() && *n >= 0.0))
        .map(|parts| parts.iter().fold(0.0, |total, part| total * 60.0 + part))
        .filter(|n| *n < 60.0 * 60.0 * 24.0 * 30.0)
        .unwrap_or(0.0)
}

pub fn parse(bytes: &[u8], source: &str) -> Result<ParsedFeed, String> {
    let channel = rss::Channel::read_from(bytes)
        .map_err(|_| "Please provide a podcast RSS feed, not a webpage or audio URL".to_owned())?;
    let base = url(source)?;
    let resolve = |value: &str| {
        base.join(value)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .map(String::from)
    };
    let itunes = channel.itunes_ext();
    let title = plain(channel.title());
    if title.is_empty() {
        return Err("This RSS feed has no show title".into());
    }
    let feed = Feed {
        url: base.to_string(),
        title,
        author: itunes
            .and_then(|i| i.author())
            .map(plain)
            .unwrap_or_default(),
        description: plain(channel.description()),
        image_url: itunes
            .and_then(|i| i.image())
            .or_else(|| channel.image().map(|i| i.url()))
            .and_then(resolve),
        subscribed: true,
        ..Feed::default()
    };
    let episodes = channel
        .items()
        .iter()
        .take(20_000)
        .map(|item| {
            let enclosure = item.enclosure();
            let media_url = enclosure
                .filter(|e| {
                    e.mime_type().starts_with("audio/")
                        || matches!(e.mime_type(), "application/octet-stream" | "")
                })
                .and_then(|e| resolve(e.url()));
            Episode {
                guid: item
                    .guid()
                    .map(|g| g.value().trim().to_owned())
                    .filter(|g| !g.is_empty()),
                title: item
                    .title()
                    .map(plain)
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "Untitled episode".into()),
                description: plain(item.content().or(item.description()).unwrap_or("")),
                media_url,
                mime: enclosure
                    .map(|e| e.mime_type().to_owned())
                    .unwrap_or_default(),
                published: item
                    .pub_date()
                    .and_then(|v| chrono::DateTime::parse_from_rfc2822(v).ok())
                    .map(|v| v.timestamp()),
                duration: item
                    .itunes_ext()
                    .and_then(|i| i.duration())
                    .map(duration)
                    .unwrap_or(0.0),
                ..Episode::default()
            }
        })
        .collect();
    Ok(ParsedFeed {
        feed,
        episodes,
        aliases: vec![base.to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_namespaces_entities_and_optional_metadata() {
        let xml = br#"<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel><title>A &amp; B</title><link>https://example.org</link><description><![CDATA[<p>hello &amp; goodbye</p>]]></description><itunes:author>Host</itunes:author><itunes:image href="/cover.png"/><item><title>Episode</title><guid isPermaLink="false">opaque</guid><enclosure url="/episode.mp3?token=secret" type="audio/mpeg" length="12"/><itunes:duration>1:02:03</itunes:duration></item><item><title>No audio</title></item></channel></rss>"#;
        let parsed = parse(xml, "https://example.org/feed?token=private").unwrap();
        assert_eq!(parsed.feed.title, "A & B");
        assert_eq!(parsed.feed.description, "hello & goodbye");
        assert_eq!(parsed.feed.author, "Host");
        assert_eq!(
            parsed.feed.image_url.as_deref(),
            Some("https://example.org/cover.png")
        );
        assert_eq!(parsed.episodes[0].duration, 3723.0);
        assert_eq!(parsed.episodes[0].guid.as_deref(), Some("opaque"));
        assert!(
            parsed.episodes[0]
                .media_url
                .as_ref()
                .unwrap()
                .ends_with("token=secret")
        );
        assert!(parsed.episodes[1].media_url.is_none());
        assert!(url("file:///etc/passwd").is_err());
        assert!(parse(b"<html>webpage</html>", "https://example.org").is_err());
        assert_eq!(duration("NaN"), 0.0);
        assert_eq!(plain("a<script>bad()</script><br>b"), "a\nb");
    }
}
